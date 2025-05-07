use proc_macro::TokenStream;
use quote::{format_ident, quote};
use std::str::FromStr;
use syn::{parse_macro_input, Lit, Meta, MetaNameValue};

macro_rules! litstr {
    ($lit: expr) => {
        if let Lit::Str(s) = $lit {
            s.value()
        } else {
            panic!("invalid string value")
        }
    };
}

/// Sorting for structures
///
/// Automatically implements Eq, PartialEq, Ord and PartialOrd for single-field comparison,
/// supports structures with no or a single lifetime.
///
/// The default sorting field is "id", can be overriden with sorting(id = "field") attribute:
///
/// # Panics
///
/// Will panic on invalid attributes and if the expression is not a struct
///
/// ```rust
/// use bmart_derive::Sorting;
///
/// #[derive(Sorting)]
/// #[sorting(id = "name")]
/// struct MyStruct {
///     name: String,
///     value: u32
/// }
/// ```
#[proc_macro_derive(Sorting, attributes(sorting))]
pub fn sorting_derive(input: TokenStream) -> TokenStream {
    let sitem = parse_macro_input!(input as syn::ItemStruct);
    let sid = &sitem.ident;
    let mut owned = true;
    for param in sitem.generics.params {
        if let syn::GenericParam::Lifetime(_) = param {
            owned = false;
            break;
        }
    }
    let mut id = "id".to_owned();
    for a in &sitem.attrs {
        if a.path.is_ident("sorting") {
            if let Ok(nameval) = a.parse_args::<MetaNameValue>() {
                if nameval.path.is_ident("id") {
                    id = litstr!(nameval.lit);
                } else {
                    panic!("invalid attribute")
                }
            } else {
                panic!("invalid attribute")
            }
        }
    }
    let i_id = format_ident!("{}", id);
    let tr = if owned {
        quote! {
            impl Eq for #sid {}
            impl Ord for #sid {
                fn cmp(&self, other: &Self) -> ::std::cmp::Ordering {
                    self.#i_id.cmp(&other.#i_id)
                }
            }
            impl PartialOrd for #sid {
                fn partial_cmp(&self, other: &Self) -> Option<::std::cmp::Ordering> {
                    Some(self.cmp(other))
                }
            }
            impl PartialEq for #sid {
                fn eq(&self, other: &Self) -> bool {
                    self.#i_id == other.#i_id
                }
            }
        }
    } else {
        quote! {
            impl<'srt> Eq for #sid<'srt> {}
            impl<'srt> Ord for #sid<'srt> {
                fn cmp(&self, other: &Self) -> ::std::cmp::Ordering {
                    self.#i_id.cmp(&other.#i_id)
                }
            }
            impl<'srt> PartialOrd for #sid<'srt> {
                fn partial_cmp(&self, other: &Self) -> Option<::std::cmp::Ordering> {
                    Some(self.cmp(other))
                }
            }
            impl<'srt> PartialEq for #sid<'srt> {
                fn eq(&self, other: &Self) -> bool {
                    self.#i_id == other.#i_id
                }
            }
        }
    };
    TokenStream::from(tr)
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum Case {
    Lower,
    Upper,
    Snake,
    ScrSnake,
    Kebab,
    ScrKebab,
    Camel,
}

impl FromStr for Case {
    type Err = ::std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "lowercase" => Case::Lower,
            "UPPERCASE" => Case::Upper,
            "snake_case" => Case::Snake,
            "SCREAMING_SNAKE_CASE" => Case::ScrSnake,
            "kebab-case" => Case::Kebab,
            "SCREAMING-KEBAB-CASE" => Case::ScrKebab,
            "CamelCase" => Case::Camel,
            _ => panic!("unsupported case: {}", s),
        })
    }
}

fn format_case(s: &str, case: Case) -> String {
    match case {
        Case::Camel => s.to_owned(),
        Case::Lower => s.to_lowercase(),
        Case::Upper => s.to_uppercase(),
        Case::Snake | Case::ScrSnake | Case::Kebab | Case::ScrKebab => {
            let sep = if case == Case::Snake || case == Case::ScrSnake {
                "_"
            } else {
                "-"
            };
            let mut result = String::new();
            for c in s.chars() {
                if c.is_uppercase() && !result.is_empty() {
                    result += sep;
                }
                result.push(c);
            }
            if case == Case::Snake || case == Case::Kebab {
                result.to_lowercase()
            } else {
                result.to_uppercase()
            }
        }
    }
}

struct EnumVar {
    id: String,
    name: Option<String>,
    aliases: Vec<String>,
    skip: bool,
}

impl EnumVar {
    fn new(i: &syn::Ident) -> Self {
        Self {
            id: i.to_string(),
            name: None,
            aliases: Vec::new(),
            skip: false,
        }
    }
}

/// Implements Display and FromStr for enums with no data attached. The default behavior is to use
/// snake_case. Can be overriden with enumstr(rename_all = "case")
///
/// The possible case values: "lowercase", "UPPERCASE", "snake_case", "SCREAMING_SNAKE_CASE",
/// "kebab-case", "SCREAMING-KEBAB-CASE". "CamelCase" (as-is)
///
/// Individual fields can be overriden with enumstr(rename = "name"), altered with enumstr(alias =
/// "alias")
///
/// Fields, marked with enumstr(skip), are skipted in FromStr implementation.
///
/// To avoid additional dependancies, parse() Err type is String.
///
/// # Panics
///
/// Will panic on invalid attributes and if the expression is not an enum
///
/// ```rust
/// use bmart_derive::EnumStr;
///
/// #[derive(EnumStr)]
/// #[enumstr(rename_all = "snake_case")]
/// enum MyEnum {
///     Field1,
///     Field2,
///     #[enumstr(skip)]
///     SecretField,
///     VeryLongField,
///     #[enumstr(rename = "another")]
///     #[enumstr(alias = "a")]
///     #[enumstr(alias = "af")]
///     AnotherField
/// }
/// ```
#[proc_macro_derive(EnumStr, attributes(enumstr))]
pub fn enumstr_derive(input: TokenStream) -> TokenStream {
    let sitem = parse_macro_input!(input as syn::ItemEnum);
    let mut vars: Vec<EnumVar> = Vec::new();
    for var in &sitem.variants {
        let mut evar = EnumVar::new(&var.ident);
        for a in &var.attrs {
            if a.path.is_ident("enumstr") {
                if let Ok(nameval) = a.parse_args::<MetaNameValue>() {
                    if nameval.path.is_ident("rename") {
                        evar.name = Some(litstr!(nameval.lit));
                    } else if nameval.path.is_ident("alias") {
                        evar.aliases.push(litstr!(nameval.lit));
                    } else {
                        panic!("invalid attribute")
                    }
                } else if let Ok(name) = a.parse_args::<Meta>() {
                    if name.path().is_ident("skip") {
                        evar.skip = true;
                    } else {
                        panic!("invalid attribute")
                    }
                } else {
                    panic!("invalid attribute")
                }
            }
        }
        vars.push(evar);
    }
    let sid = &sitem.ident;
    let mut case = Case::Snake;
    for a in &sitem.attrs {
        if a.path.is_ident("enumstr") {
            if let Ok(nameval) = a.parse_args::<MetaNameValue>() {
                if nameval.path.is_ident("rename_all") {
                    case = litstr!(nameval.lit).parse().unwrap();
                } else {
                    panic!("invalid attribute")
                }
            } else {
                panic!("invalid attribute")
            }
        }
    }
    let mut st_to = "match self {".to_owned();
    let mut st_from = "match s {".to_owned();
    for var in vars {
        let name = if let Some(name) = var.name {
            name
        } else {
            format_case(&var.id, case)
        };
        st_to += &format!("{}::{} => \"{}\",", sid, var.id, name);
        if !var.skip {
            st_from += &format!("\"{}\"", name);
            for alias in var.aliases {
                st_from += &format!(" | \"{}\"", alias);
            }
            st_from += &format!(" => Ok({}::{}),", sid, var.id);
        }
    }
    st_to += "}";
    st_from += "_ => Err(\"value unsupported: \".to_owned() + s)}";
    let m_to: syn::ExprMatch = syn::parse_str(&st_to).unwrap();
    let m_from: syn::ExprMatch = syn::parse_str(&st_from).unwrap();
    let tr = quote! {
        impl ::std::fmt::Display for #sid {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                write!(f, "{}", #m_to)
            }
        }
        impl ::std::str::FromStr for #sid {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                #m_from
            }
        }
    };
    TokenStream::from(tr)
}
