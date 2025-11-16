use anchor::*;
use lazy_static::lazy_static;
use std::{
    env,
    io::{Read, Write},
    os::unix::net::UnixStream,
    os::unix::io::RawFd,
    path::PathBuf,
    process::{self, Command},
    sync::{Arc, Mutex},
    time::Duration,
};
use tempfile::TempDir;

lazy_static! {
    static ref SHUTDOWN_STATE: Mutex<bool> = Mutex::new(false);
}

// Helper function to check if we're in shutdown state (for generated code)
pub fn is_in_shutdown() -> bool {
    *SHUTDOWN_STATE.lock().unwrap()
}

klipper_config_generate!(transport = crate::TRANSPORT_OUTPUT: crate::BufferTransportOutput);

struct KlipperInstance {
    _temp_dir: TempDir,
    child: process::Child,
    socket_path: PathBuf,
}

impl KlipperInstance {
    fn new(cfg: impl AsRef<str>) -> Self {
        let klipper_path = env::var("KLIPPER_PATH")
            .expect("set `KLIPPER_PATH` environment variable to klipper path");
        let klipper_path = std::fs::canonicalize(klipper_path).expect("Klipper not found");

        let temp_dir = TempDir::new().expect("Could not create work directory");
        let cfg_filename = temp_dir.path().join("klippy.cfg");
        let socket_path = temp_dir.path().join("klippy_uds");
        
        {
            let mut cfg_file =
                std::fs::File::create(&cfg_filename).expect("Could not open config file");
            cfg_file
                .write_all(cfg.as_ref().as_bytes())
                .expect("Could not write config file");
        }

        // Start Klippy with Unix socket API enabled
        let child = Command::new("python3")
            .current_dir(&klipper_path)
            .arg("klippy/klippy.py")
            .arg(&cfg_filename)
            .arg("-a")  // Enable API server
            .arg(socket_path.to_str().unwrap())  // Socket path
            .spawn()
            .expect("Could not launch klippy");

        KlipperInstance {
            _temp_dir: temp_dir,
            child,
            socket_path,
        }
    }

    fn send_command(&self, method: &str, params: Option<serde_json::Value>) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        // Wait for socket to be available
        let mut attempts = 0;
        while !self.socket_path.exists() && attempts < 50 {
            std::thread::sleep(Duration::from_millis(100));
            attempts += 1;
        }

        if !self.socket_path.exists() {
            return Err(format!("Socket not found at {:?}", self.socket_path).into());
        }

        let mut stream = UnixStream::connect(&self.socket_path)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "id": 1,
            "params": params.unwrap_or(serde_json::json!({}))
        });

        let request_str = serde_json::to_string(&request)?;
        stream.write_all(request_str.as_bytes())?;
        stream.write_all(&[0x03])?;  // Klippy API requires 0x03 terminator
        stream.flush()?;

        let mut response_bytes = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,  // EOF
                Ok(n) => {
                    response_bytes.extend_from_slice(&buf[..n]);
                    // Check if we've received the terminator
                    if response_bytes.ends_with(&[0x03]) {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Timeout - try to parse what we have
                    break;
                }
                Err(e) => return Err(e.into()),
            }
        }

        // Remove terminator if present
        if response_bytes.ends_with(&[0x03]) {
            response_bytes.pop();
        }

        let response_str = String::from_utf8(response_bytes)?;
        let response: serde_json::Value = serde_json::from_str(&response_str)?;
        
        if let Some(error) = response.get("error") {
            return Err(format!("Klippy error: {}", error).into());
        }

        Ok(response.get("result").cloned().unwrap_or(serde_json::Value::Null))
    }

    fn emergency_stop(&self) -> Result<(), Box<dyn std::error::Error>> {
        eprintln!("Sending emergency_stop command to Klippy...");
        self.send_command("emergency_stop", None)?;
        Ok(())
    }

    fn firmware_restart(&self) -> Result<(), Box<dyn std::error::Error>> {
        eprintln!("Sending firmware_restart command to Klippy...");
        // firmware_restart is a gcode command, not a direct API method
        self.send_command("gcode/script", Some(serde_json::json!({
            "script": "FIRMWARE_RESTART"
        })))?;
        Ok(())
    }
}

impl Drop for KlipperInstance {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

struct SerialEmulator {
    master: RawFd,
    slave: RawFd,
}

impl SerialEmulator {
    fn new() -> Self {
        use nix::sys::termios::*;

        let termios: Termios = unsafe { std::mem::zeroed() };

        let ptys = nix::pty::openpty(None, &Some(termios)).expect("Could not allocate pty");

        SerialEmulator {
            master: ptys.master,
            slave: ptys.slave,
        }
    }

    fn ttyname(&self) -> PathBuf {
        nix::unistd::ttyname(self.slave).expect("Could not get TTY name")
    }

    fn master(&self) -> RawFd {
        self.master
    }
}

impl Drop for SerialEmulator {
    fn drop(&mut self) {
        let _ = nix::unistd::close(self.master);
        let _ = nix::unistd::close(self.slave);
    }
}

static TRANSPORT_OUTPUT_MUTEX: Mutex<Option<RawFd>> = Mutex::new(None);

#[derive(Debug, Default)]
struct BufferTransportOutput;

impl TransportOutput for BufferTransportOutput {
    type Output = ScratchOutput;
    fn output(&self, f: impl FnOnce(&mut Self::Output)) {
        if let Some(fd) = TRANSPORT_OUTPUT_MUTEX.lock().unwrap().as_ref() {
            let mut scratch = ScratchOutput::new();
            f(&mut scratch);
            let result = scratch.result();
            if !result.is_empty() {
                let n = nix::unistd::write(*fd, result).expect("Could not write");
                if n != result.len() {
                    panic!("Could not write full message");
                }
            }
        }
    }
}

pub(crate) const TRANSPORT_OUTPUT: BufferTransportOutput = BufferTransportOutput;

fn main() {
    let serial = SerialEmulator::new();
    *TRANSPORT_OUTPUT_MUTEX.lock().unwrap() = Some(serial.master());

    for i in 0..=(Pins::max_variant() as u8) {
        let p: Result<Pins, _> = i.try_into();
        match p {
            Err(_) => panic!("Can't map pin {i}"),
            Ok(p) => {
                if i != <Pins as Into<u8>>::into(p) {
                    panic!("Can't reverse map pin {i}")
                }
            }
        }
    }

    let instance = KlipperInstance::new(format!(
        r#"
            [mcu]
            serial: {}

            [printer]
            kinematics: none
            max_velocity: 100
            max_accel: 100
        "#,
        serial.ttyname().display()
    ));

    // Wait for Klippy to initialize and connect to MCU
    // We'll run the test sequence in a separate thread after Klippy is ready
    let instance_arc = Arc::new(instance);
    let instance_clone = Arc::clone(&instance_arc);
    std::thread::spawn(move || {
        // Wait for socket to be available and Klippy to be ready
        eprintln!("Waiting for Klippy to initialize...");
        std::thread::sleep(Duration::from_secs(3));
        
        // Wait a bit more for MCU connection to be established
        std::thread::sleep(Duration::from_secs(2));
        
        // Test sequence: emergency_stop -> firmware_restart -> validate recovery
        eprintln!("Starting test sequence...");
        
        // Step 1: Send emergency_stop command
        if let Err(e) = instance_clone.emergency_stop() {
            eprintln!("Failed to send emergency_stop: {}", e);
        } else {
            eprintln!("Emergency stop command sent, waiting for MCU to enter shutdown state...");
            std::thread::sleep(Duration::from_secs(1));
            
            // Validate shutdown state
            if is_in_shutdown() {
                eprintln!("✓ MCU is in shutdown state");
            } else {
                eprintln!("✗ MCU is NOT in shutdown state (unexpected)");
            }
            
            // Step 2: Send firmware_restart command
            std::thread::sleep(Duration::from_secs(1));
            if let Err(e) = instance_clone.firmware_restart() {
                eprintln!("Failed to send firmware_restart: {}", e);
            } else {
                eprintln!("Firmware restart command sent, waiting for MCU to recover...");
                std::thread::sleep(Duration::from_secs(2));
                
                // Validate recovery
                if !is_in_shutdown() {
                    eprintln!("✓ MCU recovered from shutdown state");
                } else {
                    eprintln!("✗ MCU is still in shutdown state (unexpected)");
                }
            }
        }
    });

    let mut recv = [0u8; 128];
    let mut rcvbuf: Vec<u8> = Vec::new();
    loop {
        match nix::unistd::read(serial.master(), &mut recv) {
            Err(nix::errno::Errno::EWOULDBLOCK) => {}
            Err(e) => panic!("read failed: {e})"),
            Ok(n) => {
                rcvbuf.extend(&recv[..n]);
                
                // Normal operation - errors are handled in receive
                // The dispatch function will filter commands in shutdown state
                if let Err(e) = KLIPPER_TRANSPORT.receive(&mut rcvbuf, ()) {
                    match e {
                        crate::_anchor_config::KlipperCommandError::EmergencyStop(_) => {
                            eprintln!("Emergency stop triggered! Entering shutdown state.");
                            // Shutdown state is already set by emergency_stop command
                            // Continue loop to process clear_shutdown/reset commands
                        }
                        crate::_anchor_config::KlipperCommandError::GetConfig(_) => {
                            // ConfigError::NotFound is expected when config hasn't been set yet
                            eprintln!("Config error (expected if config not set): {:?}", e);
                            // Continue loop - don't break
                        }
                        _ => {
                            eprintln!("Command error: {:?}", e);
                            break;
                        }
                    }
                }
            }
        };
        if cur_clock() > 10 * CLOCK_FREQ {
            klipper_output!("This the %uth test! %*s?", Pins::PB8.into(), "You alright?");
            klipper_shutdown!("This is a test!", cur_clock());
        }
    }
}

fn cur_clock() -> u32 {
    use std::time::Instant;
    lazy_static! {
        static ref BEGIN: Instant = Instant::now();
    }
    let c = (BEGIN.elapsed().as_secs_f64() * (CLOCK_FREQ as f64)).floor() as u64;
    (c & 0xFFFFFFFF) as u32
}

#[klipper_command]
fn get_uptime(_context: &()) {
    klipper_reply!(uptime, high: u32 = 2, clock: u32 = cur_clock());
}

#[klipper_command]
fn get_clock() {
    klipper_reply!(clock, clock: u32 = cur_clock());
}

#[derive(Debug)]
pub enum EmergencyStopError {
    Triggered,
}

#[klipper_command]
fn emergency_stop() -> Result<(), EmergencyStopError> {
    *SHUTDOWN_STATE.lock().unwrap() = true;
    Err(EmergencyStopError::Triggered)
}

lazy_static! {
    static ref CONFIG_CRC: Mutex<Option<u32>> = Mutex::new(None);
}

#[derive(Debug)]
pub enum ConfigError {
    NotFound,
}

#[klipper_command]
fn get_config() -> Result<(), ConfigError> {
    let crc = CONFIG_CRC.lock().unwrap();

    klipper_reply!(
        config,
        is_config: bool = crc.is_some(),
        crc: u32 = crc.unwrap_or(0),
        is_shutdown: bool = false,
        move_count: u16 = 0
    );
    if crc.is_none() {
        return Err(ConfigError::NotFound);
    }
    Ok(())
}

#[klipper_command]
fn config_reset() {
    *CONFIG_CRC.lock().unwrap() = None;
    *SHUTDOWN_STATE.lock().unwrap() = false;
}

#[klipper_command]
fn clear_shutdown() {
    *SHUTDOWN_STATE.lock().unwrap() = false;
}

#[klipper_command]
fn reset() {
    *SHUTDOWN_STATE.lock().unwrap() = false;
}

#[klipper_command]
fn finalize_config(crc: u32) {
    *CONFIG_CRC.lock().unwrap() = Some(crc);
}

#[klipper_command]
fn allocate_oids(count: u8) {
    let _ = count;
}

#[klipper_command]
fn test_array(buf: &[u8], offset: u16) {
    let _ = buf;
    let _ = offset;
}

#[klipper_command]
#[cfg(feature = "skipped_command")]
fn must_skip() {
    klipper_output!("Output in a skipped command!");
    klipper_reply!(reply_in_skipped);
}

#[klipper_constant]
const CLOCK_FREQ: u32 = 100_000_000;

#[klipper_constant]
const MCU: &str = "anchor_jig";

#[klipper_constant]
const STATS_SUMSQ_BASE: u32 = 256;

#[cfg(feature = "skipped_command")]
#[klipper_constant]
const TEST: &str = "skipped most of the time";

klipper_enumeration! {
    #[derive(Debug)]
    #[klipper_enumeration(name = "spi_bus", rename_all = "snake_case")]
    #[allow(dead_code)]
    enum SpiBus {
        #[klipper_enumeration(rename = "spi0a")]
        Spi0A,
        #[klipper_enumeration(rename = "spi0b")]
        Spi0B,
        Spi0C,
        Spi0D,
        Spi1A,
        Spi1B,
        #[cfg(feature="skipped_command")]
        Spi1C,
    }
}

klipper_enumeration! {
    #[derive(Debug, Copy, Clone, Eq, PartialEq)]
    #[klipper_enumeration(name = "pin", rename_all = "UPPERCASE")]
    enum Pins {
        Range(PA, 0, 16),
        Range(PB, 0, 16),
        AdcTemperature,
    }
}

mod test_embed {
    use anchor::*;
    #[klipper_command]
    pub fn woot() {}
}

mod test;

#[cfg(feature = "skipped_command")]
mod test_skipped {
    use anchor::*;
    #[klipper_command]
    pub fn skipped_command_in_module() {}
}

#[klipper_command]
fn wee() {}
