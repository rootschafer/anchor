use bitflags::bitflags;

bitflags! {
    /// Flags that can be set on Klipper commands to control their behavior
    ///
    /// These flags can be used with the `#[klipper_command(flags = ...)]` attribute
    /// to modify how commands are handled by the protocol layer.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use anchor::KlipperCommandFlags;
    ///
    /// #[klipper_command(flags = KlipperCommandFlags::HF_IN_SHUTDOWN)]
    /// fn clear_shutdown() {
    ///     // This command can execute during shutdown state
    /// }
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct KlipperCommandFlags: u32 {
        /// Command is allowed to execute during shutdown state
        ///
        /// When the `shutdown-filtering` feature is enabled, commands without this flag
        /// will be automatically filtered out when the system is in shutdown state.
        /// This allows only recovery commands (like `clear_shutdown`, `config_reset`)
        /// to execute, matching Klipper's firmware behavior.
        const HF_IN_SHUTDOWN = 1 << 0;
    }
}

