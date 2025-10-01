use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::{encoding::*, input_buffer::InputBuffer, output_buffer::OutputBuffer, transport_output::TransportOutput};

#[cfg(feature = "async")]
use embassy_sync::channel::Channel;

const MESSAGE_HEADER_SIZE: usize = 2;
const MESSAGE_TRAILER_SIZE: usize = 3;
const MESSAGE_LENGTH_MIN: usize = MESSAGE_HEADER_SIZE + MESSAGE_TRAILER_SIZE;
const MESSAGE_LENGTH_MAX: usize = 64;
const MESSAGE_POSITION_LENGTH: usize = 0;
const MESSAGE_POSITION_SEQ: usize = 1;
const MESSAGE_TRAILER_CRC: usize = 3;
const MESSAGE_TRAILER_SYNC: usize = 1;
const MESSAGE_VALUE_SYNC: u8 = 0x7E;
const MESSAGE_DEST: u8 = 0x10;
const MESSAGE_SEQ_MASK: u8 = 0x0F;

fn crc16(buf: &[u8]) -> u16 {
	let mut crc = 0xFFFFu16;
	for b in buf {
		let b = *b ^ ((crc & 0xFF) as u8);
		let b = b ^ (b << 4);
		let b16 = b as u16;
		crc = (b16 << 8 | crc >> 8) ^ (b16 >> 4) ^ (b16 << 3);
	}
	crc
}

pub trait Config {
	type TransportOutput: TransportOutput;
	type Context<'c>;
	
	#[cfg(not(feature = "async"))]
	fn dispatch<'c>(cmd: u16, frame: &mut &[u8], context: &mut Self::Context<'c>) -> Result<(), ReadError>;
	
	#[cfg(feature = "async")]
	fn dispatch<'c>(cmd: u16, frame: &mut &'c [u8], context: &'c mut Self::Context<'c>) -> impl core::future::Future<Output = Result<(), ReadError>> + 'c;
}

/// Protocol transport implementation
pub struct Transport<C: Config + 'static> {
	is_synchronized: AtomicBool,
	next_sequence: AtomicU8,
	output: C::TransportOutput,
}

impl<C: Config> Transport<C> {
	#[doc(hidden)]
	pub const fn new(_config: &'static C, output: C::TransportOutput) -> Self {
		Self {
			is_synchronized: AtomicBool::new(true),
			next_sequence: AtomicU8::new(MESSAGE_DEST),
			output,
		}
	}

	/// Decodes messages from an `InputBuffer`
	#[cfg(not(feature = "async"))]
	pub fn receive<'c>(&self, input: &mut impl InputBuffer, mut context: C::Context<'c>) {
		// Drive state machine forward until we either have no
		// input or know we don't have enough input.
		let mut data = input.data();
		while !data.is_empty() {
			if !self.is_synchronized.load(Ordering::SeqCst) {
				// Look for a sync byte
				if let Some(n) = data.iter().position(|b| *b == MESSAGE_VALUE_SYNC) {
					data = &data[n + 1..];
					self.is_synchronized.store(true, Ordering::SeqCst);
					self.encode_acknak();
				} else {
					data = &[];
				}
			} else {
				if data[0] == MESSAGE_VALUE_SYNC {
					data = &data[1..];
					continue;
				}

				if data.len() < MESSAGE_LENGTH_MIN {
					break;
				}

				let len = data[MESSAGE_POSITION_LENGTH] as usize;
				if !(MESSAGE_LENGTH_MIN..=MESSAGE_LENGTH_MAX).contains(&len) {
					self.is_synchronized.store(false, Ordering::SeqCst);
					continue;
				}

				let seq = data[MESSAGE_POSITION_SEQ];
				if seq & !MESSAGE_SEQ_MASK != MESSAGE_DEST {
					self.is_synchronized.store(false, Ordering::SeqCst);
					continue;
				}
				if data.len() < len {
					break;
				}
				if data[len - MESSAGE_TRAILER_SYNC] != MESSAGE_VALUE_SYNC {
					self.is_synchronized.store(false, Ordering::SeqCst);
					continue;
				}

				let frame_crc =
					((data[len - MESSAGE_TRAILER_CRC] as u16) << 8) | (data[len - MESSAGE_TRAILER_CRC + 1] as u16);
				let actual_crc = crc16(&data[0..len - MESSAGE_TRAILER_SIZE]);
				if frame_crc != actual_crc {
					self.is_synchronized.store(false, Ordering::SeqCst);
					continue;
				}

				let frame = &data[MESSAGE_HEADER_SIZE..len - MESSAGE_TRAILER_SIZE];
				data = &data[len..];
				if seq == self.next_sequence.load(Ordering::SeqCst) {
					self.next_sequence
						.store(((seq + 1) & MESSAGE_SEQ_MASK) | MESSAGE_DEST, Ordering::SeqCst);
					let _ = self.parse_frame(frame, &mut context);
				}
				self.encode_acknak();
			}
		}
		// Remove consumed bytes from front
		let consumed = input.available() - data.len();
		if consumed > 0 {
			input.pop(consumed);
		}
	}
	
	#[cfg(not(feature = "async"))]
	fn parse_frame<'c>(&self, mut frame: &[u8], context: &mut C::Context<'c>) -> Result<(), ReadError> {
		while !frame.is_empty() {
			let cmd = <u16 as Readable>::read(&mut frame)?;
			C::dispatch(cmd, &mut frame, context)?;
		}
		Ok(())
	}
	
	#[cfg(feature = "async")]
	async fn parse_frame<'c>(&self, frame: &'c [u8], context: &'c mut C::Context<'c>) -> Result<(), ReadError> {
		let mut frame_mut = frame;
		while !frame_mut.is_empty() {
			let cmd = <u16 as Readable>::read(&mut frame_mut)?;
			C::dispatch(cmd, &mut frame_mut, context).await?;
		}
		Ok(())
	}

	/// Async receive using a channel-based pattern
	/// 
	/// This method processes incoming byte chunks from a channel and dispatches
	/// commands asynchronously. This is the recommended way to use anchor with
	/// Embassy and async/await.
	/// 
	/// # Example
	/// ```ignore
	/// // UART task sends data to channel
	/// #[embassy_executor::task]
	/// async fn uart_rx_task(channel: &'static Channel<NoopRawMutex, [u8; 64], 4>) {
	///     loop {
	///         let mut buf = [0u8; 64];
	///         let len = uart.read(&mut buf).await.unwrap();
	///         channel.send(buf[..len].to_owned()).await;
	///     }
	/// }
	/// 
	/// // Transport task processes from channel
	/// transport.receive_from_channel(channel, &mut context).await;
	/// ```
	#[cfg(feature = "async")]
	pub async fn receive_from_bytes<'c>(&self, bytes: &'c [u8], context: &'c mut C::Context<'c>) {
		if bytes.is_empty() {
			return;
		}

		let mut data = bytes;
		while !data.is_empty() {
			if !self.is_synchronized.load(Ordering::SeqCst) {
				// Look for a sync byte
				if let Some(n) = data.iter().position(|b| *b == MESSAGE_VALUE_SYNC) {
					data = &data[n + 1..];
					self.is_synchronized.store(true, Ordering::SeqCst);
					self.encode_acknak();
				} else {
					return;
				}
			} else {
				if data[0] == MESSAGE_VALUE_SYNC {
					data = &data[1..];
					continue;
				}

				if data.len() < MESSAGE_LENGTH_MIN {
					return;
				}

				let len = data[MESSAGE_POSITION_LENGTH] as usize;
				if !(MESSAGE_LENGTH_MIN..=MESSAGE_LENGTH_MAX).contains(&len) {
					self.is_synchronized.store(false, Ordering::SeqCst);
					continue;
				}

				let seq = data[MESSAGE_POSITION_SEQ];
				if seq & !MESSAGE_SEQ_MASK != MESSAGE_DEST {
					self.is_synchronized.store(false, Ordering::SeqCst);
					continue;
				}
				if data.len() < len {
					return;
				}
				if data[len - MESSAGE_TRAILER_SYNC] != MESSAGE_VALUE_SYNC {
					self.is_synchronized.store(false, Ordering::SeqCst);
					continue;
				}

				let frame_crc =
					((data[len - MESSAGE_TRAILER_CRC] as u16) << 8) | (data[len - MESSAGE_TRAILER_CRC + 1] as u16);
				let actual_crc = crc16(&data[0..len - MESSAGE_TRAILER_SIZE]);
				if frame_crc != actual_crc {
					self.is_synchronized.store(false, Ordering::SeqCst);
					continue;
				}

				let frame = &data[MESSAGE_HEADER_SIZE..len - MESSAGE_TRAILER_SIZE];
				if seq == self.next_sequence.load(Ordering::SeqCst) {
					self.next_sequence
						.store(((seq + 1) & MESSAGE_SEQ_MASK) | MESSAGE_DEST, Ordering::SeqCst);
					let _ = self.parse_frame(frame, context).await;
				}
				self.encode_acknak();
				data = &data[len..];
			}
		}
	}

	// Fast path for ACK/NAK
	fn encode_acknak(&self) {
		self.output.output(|output| {
			let ns = self.next_sequence.load(Ordering::SeqCst);
			let crc = crc16(&[5, ns]);
			output.output(&[5, ns, ((crc & 0xFF00) >> 8) as u8, (crc & 0xFF) as u8, MESSAGE_VALUE_SYNC]);
		});
	}

	#[doc(hidden)]
	pub fn encode_frame(&self, f: impl FnOnce(&mut <<C as Config>::TransportOutput as TransportOutput>::Output)) {
		self.output.output(|output| {
			let cursor = output.cur_position();
			output.output(&[0, self.next_sequence.load(Ordering::SeqCst)]); // Output header
			f(output); // Output actual frame contents
			{
				let changed = output.data_since(cursor).len();
				output.update(cursor, (changed + MESSAGE_TRAILER_SIZE) as u8);
			}
			let crc = crc16(output.data_since(cursor));
			output.output(&[((crc & 0xFF00) >> 8) as u8, (crc & 0xFF) as u8, MESSAGE_VALUE_SYNC]);
		})
	}
}
