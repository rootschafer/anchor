use std::fmt::Display;

use quote::{format_ident, IdentFragment};
use syn::{parse::Parse, Expr, Ident, LitStr, Token};

#[derive(Debug)]
pub(crate) struct HexName<'a>(pub &'a str, pub bool);

impl<'a> Display for HexName<'a> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let base = if self.1 { 65 } else { 97 };
		for b in self.0.bytes() {
			let high = (b >> 4) & 0xF;
			let low = b & 0xF;
			write!(f, "{}{}", (base + high) as char, (base + low) as char)?;
		}
		Ok(())
	}
}

impl<'a> IdentFragment for HexName<'a> {
	fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
		<Self as Display>::fmt(self, f)
	}
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StaticString(pub String);

impl StaticString {
	pub fn compile_name(&self) -> Ident {
		format_ident!("STATIC_STRING_{}", HexName(&self.0, true))
	}
}

impl Parse for StaticString {
	fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
		let s: LitStr = input.parse()?;
		Ok(StaticString(s.value()))
	}
}

/// Represents either a static string literal or an expression that can provide a static string
#[derive(Debug, Clone)]
pub enum ShutdownMessage {
	/// A static string literal
	StaticString(StaticString),
	/// An expression that implements StaticStringProvider trait
	Expression(Expr),
}

impl ShutdownMessage {
	pub fn compile_name(&self) -> Ident {
		match self {
			ShutdownMessage::StaticString(s) => s.compile_name(),
			ShutdownMessage::Expression(expr) => {
				// For expressions, we need to generate a name based on the expression
				// This is a bit tricky, but we can use the expression's string representation
				let expr_str = quote::quote! { #expr }.to_string();
				format_ident!("STATIC_STRING_{}", HexName(&expr_str, true))
			}
		}
	}

	pub fn is_static_string(&self) -> bool {
		matches!(self, ShutdownMessage::StaticString(_))
	}

	pub fn is_expression(&self) -> bool {
		matches!(self, ShutdownMessage::Expression(_))
	}
}

#[derive(Debug)]
pub struct Shutdown {
	pub msg: ShutdownMessage,
	pub clock: Expr,
}

impl Parse for Shutdown {
	// fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
	// 	// Try to parse as a string literal first
	// 	let lookahead = input.lookahead1();
	// 	if lookahead.peek(LitStr) {
	// 		let s: LitStr = input.parse()?;
	// 		input.parse::<Comma>()?;
	// 		let clock = input.parse()?;
	// 		Ok(Shutdown {
	// 			msg: ShutdownMessage::StaticString(StaticString(s.value())),
	// 			clock
	// 		})
	// 	} else {
	// 		// Parse as an expression
	// 		let expr: Expr = input.parse()?;
	// 		input.parse::<Comma>()?;
	// 		let clock = input.parse()?;
	// 		Ok(Shutdown {
	// 			msg: ShutdownMessage::Expression(expr),
	// 			clock
	// 		})
	// 	}
	// }

	fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
		// parse the first argument (string literal OR expression)
		let msg = if input.peek(LitStr) {
			// It's a string literal
			let s: LitStr = input.parse()?;
			ShutdownMessage::StaticString(StaticString(s.value()))
		} else {
			// It's an expression (identifier, method call, etc.)
			ShutdownMessage::Expression(input.parse::<Expr>()?)
		};

		// then expect a comma
		input.parse::<Token![,]>()?;

		// parse the clock expression (any valid Expr)
		let clock: Expr = input.parse()?;

		Ok(Shutdown { msg, clock })
	}
}
