//! Symbol name demangling for diagnostics.

/// Demangles an Itanium C++ ABI symbol name, if `name` is one.
pub fn demangle_cpp(name: &[u8]) -> Option<String> {
    if !name.starts_with(b"_Z") {
        return None;
    }
    let sym = cpp_demangle::Symbol::new(name).ok()?;
    let options = cpp_demangle::DemangleOptions::default();
    sym.demangle(&options).ok()
}

/// A Mach-O symbol name as diagnostics spell it: demangled when
/// -demangle is in effect. Mach-O prefixes every C-level name with an
/// underscore, so an Itanium name reads `__Z...` here; symbol lookup and
/// output always use the original spelling.
pub fn display_name(name: &str) -> std::borrow::Cow<'_, str> {
    if crate::error::demangle_enabled()
        && let Some(demangled) = name.strip_prefix('_').and_then(|n| demangle_cpp(n.as_bytes()))
    {
        return demangled.into();
    }
    name.into()
}
