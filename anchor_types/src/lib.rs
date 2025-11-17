use bitflags::bitflags;

bitflags! {
    /// Flags that can be set on Klipper commands to control their behavior
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct KlipperCommandFlags: u32 {
        /// Command is allowed to execute during shutdown state
        const HF_IN_SHUTDOWN = 1 << 0;
    }
}

