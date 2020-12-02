use codespan::Span;
use data_structures::stable_hasher;

static mut GLOBAL_SYMBOL_INTERNER: SymbolInterner = SymbolInterner::new();

// #[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Encode, Decode)]
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Symbol(usize);

// #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Encode, Decode)]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolData(Box<str>);

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ident {
    pub span: Span,
    pub symbol: Symbol,
}

impl Symbol {
    pub fn new(name: impl Into<Box<str>>) -> Symbol {
        unsafe { GLOBAL_SYMBOL_INTERNER.intern(SymbolData(name.into())) }
    }

    pub fn dummy() -> Symbol {
        unsafe { GLOBAL_SYMBOL_INTERNER.intern(SymbolData(Default::default())) }
    }

    pub const fn from_usize(src: usize) -> Symbol {
        Symbol(src)
    }

    pub fn as_static_str(self) -> &'static str {
        unsafe {
            let boxed_str = &GLOBAL_SYMBOL_INTERNER.data[self.0].0;
            let slice = std::slice::from_raw_parts(boxed_str.as_ptr(), boxed_str.len());

            std::str::from_utf8_unchecked(slice)
        }
    }
}

impl std::fmt::Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        <str as std::fmt::Display>::fmt(&(**self).0, f)
    }
}

impl std::ops::Deref for Symbol {
    type Target = SymbolData;

    fn deref(&self) -> &SymbolData {
        unsafe { &GLOBAL_SYMBOL_INTERNER.data[self.0] }
    }
}

impl std::ops::Deref for SymbolData {
    type Target = str;

    fn deref(&self) -> &str {
        &*self.0
    }
}

#[derive(Debug)]
pub struct SymbolInterner {
    data: Vec<SymbolData>,
}

impl SymbolInterner {
    pub const fn new() -> SymbolInterner {
        SymbolInterner { data: Vec::new() }
    }

    fn intern(&mut self, value: SymbolData) -> Symbol {
        if let Some(idx) = self.data.iter().position(|d| d == &value) {
            Symbol(idx)
        } else {
            self.data.push(value);

            Symbol(self.data.len() - 1)
        }
    }
}

impl Ident {
    pub fn dummy() -> Ident {
        Ident {
            symbol: Symbol::default(),
            span: Span::default(),
        }
    }

    pub fn to_string(&self) -> String {
        self.symbol.to_string()
    }

    pub fn peek_any(cursor: parser::buffer::Cursor) -> bool {
        cursor.ident().is_some()
    }
}

impl<D> parser::parse::Parse<D> for Ident {
    fn parse(input: parser::parse::ParseStream<D>) -> parser::error::Result<Self> {
        let ident = input.parse::<parser::ident::Ident>()?;

        Ok(Ident {
            span: ident.span,
            symbol: Symbol::new(ident.name),
        })
    }
}

impl parser::token::Token for Ident {
    fn peek(cursor: parser::buffer::Cursor) -> bool {
        match cursor.ident() {
            Some((ident, _)) => match ident.name.as_str() {
                "where" | "then" | "else" | "of" | "do" => false,
                _ => true,
            },
            None => false,
        }
    }

    fn display() -> &'static str {
        "identifier"
    }
}

impl std::fmt::Display for Ident {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        <Symbol as std::fmt::Display>::fmt(&self.symbol, f)
    }
}

impl std::hash::Hash for Ident {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (&*self.symbol).hash(state);
    }
}

impl<CTX> stable_hasher::HashStable<CTX> for Symbol {
    fn hash_stable(&self, ctx: &mut CTX, hasher: &mut stable_hasher::StableHasher) {
        (&***self).hash_stable(ctx, hasher);
    }
}

// impl serde::Serialize for Symbol {
//     fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
//         self.to_string().serialize(s)
//     }
// }
//
// impl<'de> serde::Deserialize<'de> for Symbol {
//     fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
//         let text = <String>::deserialize(d)?;
//
//         Ok(Symbol::new(text))
//     }
// }
