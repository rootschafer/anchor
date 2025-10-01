//! Traits for Anchor framework

#![no_std]

/// Trait for types that can provide a static string at compile time
///
/// This trait allows types (like enums) to be used with `klipper_shutdown!`
/// macro by providing a way to get a static string representation.
pub trait StaticStringProvider {
	/// Returns a static string representation of this value
	///
	/// This method should return a `&'static str` to ensure
	/// it can be used at compile time in macros.
	fn as_static_str(&self) -> &'static str;
}

// /// Helper trait for converting values to static strings
// ///
// /// This trait provides a more ergonomic way to convert values to static strings
// /// for use with the `klipper_shutdown!` macro.
// pub trait ToStaticString {
// 	/// Convert this value to a static string
// 	fn to_static_string(&self) -> &'static str;
// }
//
// impl<T: StaticStringProvider> ToStaticString for T {
// 	fn to_static_string(&self) -> &'static str {
// 		self.as_static_str()
// 	}
// }
