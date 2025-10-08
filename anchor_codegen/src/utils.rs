use syn::{spanned::Spanned, Attribute, Error, Expr, Lit, LitStr, Meta, MetaList};

pub fn visit_attribs(
	attrs: &[Attribute],
	ident: &str,
	mut cb: impl FnMut(&Meta) -> syn::Result<()>,
) -> syn::Result<()> {
	for attr in attrs.iter().filter(|attr| attr.path().is_ident(ident)) {
		// Parse nested meta entries
		attr.parse_nested_meta(|meta| {
			// Build a Meta from the ParseNestedMeta
			let meta_item = if meta.input.peek(syn::Token![=]) {
				// This is Meta::NameValue
				meta.input.parse::<syn::Token![=]>()?;
				let value: Expr = meta.input.parse()?;
				Meta::NameValue(syn::MetaNameValue {
					path: meta.path.clone(),
					eq_token: syn::Token![=](meta.path.span()),
					value,
				})
			} else if meta.input.peek(syn::token::Paren) {
				// This is Meta::List - but we don't handle nested parsing here
				Meta::Path(meta.path.clone())
			} else {
				// This is Meta::Path
				Meta::Path(meta.path.clone())
			};
			cb(&meta_item)?;
			Ok(())
		})?;
	}
	Ok(())
}

pub fn check_is_disabled(attrs: &[Attribute]) -> bool {
	fn check_expr(meta: &Meta) -> bool {
		match meta {
			Meta::NameValue(m) if m.path.is_ident("feature") => {
				if let Expr::Lit(expr_lit) = &m.value {
					if let Lit::Str(lit_str) = &expr_lit.lit {
						let feature = lit_str.value();
						let envname = format!("CARGO_FEATURE_{}", feature.to_uppercase().replace('-', "_"));
						return std::env::var(envname).is_err();
					}
				}
				false
			}
			Meta::List(m) if m.path.is_ident("not") => {
				// Parse first nested meta
				let mut result = true;
				let _ = m.parse_nested_meta(|nested| {
					result = !check_expr(&Meta::Path(nested.path.clone()));
					Ok(())
				});
				result
			}
			Meta::List(m) if m.path.is_ident("all") => {
				let mut all_true = true;
				let _ = m.parse_nested_meta(|nested| {
					if all_true && !check_expr(&Meta::Path(nested.path.clone())) {
						all_true = false;
					}
					Ok(())
				});
				all_true
			}
			Meta::List(m) if m.path.is_ident("any") => {
				let mut any_true = false;
				let _ = m.parse_nested_meta(|nested| {
					if !any_true && check_expr(&Meta::Path(nested.path.clone())) {
						any_true = true;
					}
					Ok(())
				});
				any_true
			}
			_ => false,
		}
	}

	let mut disabled = false;
	for attr in attrs.iter().filter(|attr| attr.path().is_ident("cfg")) {
		if let Meta::List(_) = &attr.meta {
			let _ = attr.parse_nested_meta(|meta| {
				if !disabled && check_expr(&Meta::Path(meta.path.clone())) {
					disabled = true;
				}
				Ok(())
			});
		}
	}
	disabled
}

pub fn check_is_enabled(attrs: &[Attribute]) -> bool {
	!check_is_disabled(attrs)
}

pub fn get_lit_str(lit: &Lit) -> syn::Result<&LitStr> {
	if let Lit::Str(s) = lit {
		Ok(s)
	} else {
		Err(Error::new(lit.span(), "expected attribute to be a string"))
	}
}
