use std::fs;
use std::path::PathBuf;

use genco::prelude::*;
use xshell::{cmd, Shell};

use crate::cairo_spec::get_spec;
use crate::spec::{Member, Node, NodeKind, Variant};

pub fn project_root() -> PathBuf {
    // This is the directory of Cargo.toml of the syntax_codegen crate.
    let dir = env!("CARGO_MANIFEST_DIR");
    // Pop the "/crates/cairo-lang-syntax-codegen" suffix.
    let res = PathBuf::from(dir).parent().unwrap().parent().unwrap().to_owned();
    assert!(res.join("Cargo.toml").exists(), "Could not find project root directory.");
    res
}

pub fn ensure_file_content(filename: PathBuf, content: String) {
    if let Ok(old_contents) = fs::read_to_string(&filename) {
        if old_contents == content {
            return;
        }
    }

    fs::write(&filename, content).unwrap();
}

pub fn get_codes() -> Vec<(String, String)> {
    vec![
        (
            "crates/cairo-lang-syntax/src/node/ast.rs".into(),
            reformat_rust_code(generate_ast_code().to_string().unwrap()),
        ),
        (
            "crates/cairo-lang-syntax/src/node/kind.rs".into(),
            reformat_rust_code(generate_kinds_code().to_string().unwrap()),
        ),
        (
            "crates/cairo-lang-syntax/src/node/key_fields.rs".into(),
            reformat_rust_code(generate_key_fields_code().to_string().unwrap()),
        ),
    ]
}

pub fn reformat_rust_code(text: String) -> String {
    // Since rustfmt is used with nightly features, it takes 2 runs to reach a fixed point.
    reformat_rust_code_inner(reformat_rust_code_inner(text))
}
pub fn reformat_rust_code_inner(text: String) -> String {
    let sh = Shell::new().unwrap();
    sh.set_var("RUSTUP_TOOLCHAIN", "nightly-2023-07-05");
    let rustfmt_toml = project_root().join("rustfmt.toml");
    let mut stdout = cmd!(sh, "rustfmt --config-path {rustfmt_toml}").stdin(text).read().unwrap();
    if !stdout.ends_with('\n') {
        stdout.push('\n');
    }
    stdout
}

fn generate_kinds_code() -> rust::Tokens {
    let spec = get_spec();
    let mut tokens = quote! {
        $("// Autogenerated file. To regenerate, please run `cargo run --bin generate-syntax`.\n")
        use core::fmt;
    };
    let mut kinds = rust::Tokens::new();
    let mut token_kinds = rust::Tokens::new();
    let mut keyword_token_kinds = rust::Tokens::new();
    let mut terminal_kinds = rust::Tokens::new();
    let mut keyword_terminal_kinds = rust::Tokens::new();

    // SyntaxKind.
    for Node { name, kind } in spec.iter() {
        match kind {
            NodeKind::Enum { .. } => {}
            _ => {
                kinds.extend(quote! {
                    $name,
                });
            }
        }
    }

    for Node { name, kind } in spec.into_iter() {
        match kind {
            NodeKind::Token { is_keyword } => {
                append_rust_token(&mut token_kinds, &name);
                if is_keyword {
                    append_rust_token(&mut keyword_token_kinds, &name);
                }
            }
            NodeKind::Terminal { is_keyword, .. } => {
                append_rust_token(&mut terminal_kinds, &name);
                if is_keyword {
                    append_rust_token(&mut keyword_terminal_kinds, &name);
                }
            }
            _ => {}
        }
    }

    tokens.extend(quote! {
        #[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
        pub enum SyntaxKind {
            $kinds
        }
        impl SyntaxKind {
            pub fn is_token(&self) -> bool {
                matches!(
                    *self,
                    $token_kinds
                )
            }
            pub fn is_terminal(&self) -> bool {
                matches!(
                    *self,
                    $terminal_kinds
                )
            }
            pub fn is_keyword_token(&self) -> bool {
                matches!(
                    *self,
                    $keyword_token_kinds
                )
            }
            pub fn is_keyword_terminal(&self) -> bool {
                matches!(
                    *self,
                    $keyword_terminal_kinds
                )
            }
        }
        impl fmt::Display for SyntaxKind {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{self:?}")
            }
        }
    });
    tokens
}

fn generate_key_fields_code() -> rust::Tokens {
    let spec = get_spec();
    let mut arms = rust::Tokens::new();

    for Node { name, kind } in spec.into_iter() {
        match kind {
            NodeKind::Struct { members } | NodeKind::Terminal { members, .. } => {
                let mut fields = rust::Tokens::new();
                for (i, member) in members.into_iter().enumerate() {
                    let field_name = member.name;
                    if member.key {
                        fields.extend(quote! { $("/*") $field_name $("*/") children[$i], });
                    }
                }
                arms.extend(quote! {
                    SyntaxKind::$name => {vec![$fields]},
                });
            }
            NodeKind::List { .. } | NodeKind::SeparatedList { .. } | NodeKind::Token { .. } => {
                arms.extend(quote! {
                    SyntaxKind::$name => vec![],
                });
            }
            NodeKind::Enum { .. } => {}
        }
    }
    let tokens = quote! {
        $("// Autogenerated file. To regenerate, please run `cargo run --bin generate-syntax`.\n")
        use super::ids::GreenId;
        use super::kind::SyntaxKind;

        $("/// Gets the vector of children ids that are the indexing key for this SyntaxKind.\n")
        $("/// Each SyntaxKind has some children that are defined in the spec to be its indexing key\n")
        $("/// for its stable pointer. See [super::stable_ptr].\n")
        pub fn get_key_fields(kind: SyntaxKind, children: &[GreenId]) -> Vec<GreenId> {
            match kind {
                $arms
            }
        }
    };
    tokens
}

fn generate_ast_code() -> rust::Tokens {
    let spec = get_spec();
    let mut tokens = quote! {
        $("// Autogenerated file. To regenerate, please run `cargo run --bin generate-syntax`.\n")
        #![allow(clippy::match_single_binding)]
        #![allow(clippy::too_many_arguments)]
        #![allow(dead_code)]
        #![allow(unused_variables)]
        use std::ops::Deref;
        use std::sync::Arc;

        use cairo_lang_filesystem::span::TextWidth;
        use cairo_lang_utils::extract_matches;
        use smol_str::SmolStr;

        use super::element_list::ElementList;
        use super::green::GreenNodeDetails;
        use super::kind::SyntaxKind;
        use super::{
            GreenId, GreenNode, SyntaxGroup, SyntaxNode, SyntaxStablePtr, SyntaxStablePtrId,
            Terminal, Token, TypedSyntaxNode,
        };

        #[path = "ast_ext.rs"]
        mod ast_ext;
    };
    for Node { name, kind } in spec.into_iter() {
        tokens.extend(match kind {
            NodeKind::Enum { variants, missing_variant } => {
                gen_enum_code(name, variants, missing_variant)
            }
            NodeKind::Struct { members } => gen_struct_code(name, members, false),
            NodeKind::Terminal { members, .. } => gen_struct_code(name, members, true),
            NodeKind::Token { .. } => gen_token_code(name),
            NodeKind::List { element_type } => gen_list_code(name, element_type),
            NodeKind::SeparatedList { element_type, separator_type } => {
                gen_separated_list_code(name, element_type, separator_type)
            }
        })
    }
    tokens
}

fn gen_list_code(name: String, element_type: String) -> rust::Tokens {
    // TODO(spapini): Change Deref to Borrow.
    let ptr_name = format!("{name}Ptr");
    let green_name = format!("{name}Green");
    let element_green_name = format!("{element_type}Green");
    let common_code = gen_common_list_code(&name, &green_name, &ptr_name);
    quote! {
        #[derive(Clone, Debug, Eq, Hash, PartialEq)]
        pub struct $(&name)(ElementList<$(&element_type),1>);
        impl Deref for $(&name){
            type Target = ElementList<$(&element_type),1>;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
        impl $(&name){
            pub fn new_green(
                db: &dyn SyntaxGroup, children: Vec<$(&element_green_name)>
            ) -> $(&green_name) {
                let width = children.iter().map(|id|
                    db.lookup_intern_green(id.0).width()).sum();
                $(&green_name)(db.intern_green(Arc::new(GreenNode {
                    kind: SyntaxKind::$(&name),
                    details: GreenNodeDetails::Node {
                        children: children.iter().map(|x| x.0).collect(),
                        width,
                    },
                })))
            }
        }
        #[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
        pub struct $(&ptr_name)(pub SyntaxStablePtrId);
        impl $(&ptr_name) {
            pub fn untyped(&self) -> SyntaxStablePtrId {
                self.0
            }
            pub fn lookup(&self, db: &dyn SyntaxGroup) -> $(&name) {
                $(&name)::from_syntax_node(db, self.0.lookup(db))
            }
        }
        $common_code
    }
}

fn gen_separated_list_code(
    name: String,
    element_type: String,
    separator_type: String,
) -> rust::Tokens {
    // TODO(spapini): Change Deref to Borrow.
    let ptr_name = format!("{name}Ptr");
    let green_name = format!("{name}Green");
    let element_or_separator_green_name = format!("{name}ElementOrSeparatorGreen");
    let element_green_name = format!("{element_type}Green");
    let separator_green_name = format!("{separator_type}Green");
    let common_code = gen_common_list_code(&name, &green_name, &ptr_name);
    quote! {
        #[derive(Clone, Debug, Eq, Hash, PartialEq)]
        pub struct $(&name)(ElementList<$(&element_type),2>);
        impl Deref for $(&name){
            type Target = ElementList<$(&element_type),2>;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
        impl $(&name){
            pub fn new_green(
                db: &dyn SyntaxGroup, children: Vec<$(&element_or_separator_green_name)>
            ) -> $(&green_name) {
                let width = children.iter().map(|id|
                    db.lookup_intern_green(id.id()).width()).sum();
                $(&green_name)(db.intern_green(Arc::new(GreenNode {
                    kind: SyntaxKind::$(&name),
                    details: GreenNodeDetails::Node {
                        children: children.iter().map(|x| x.id()).collect(),
                        width,
                    },
                })))
            }
        }
        #[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
        pub struct $(&ptr_name)(pub SyntaxStablePtrId);
        impl $(&ptr_name) {
            pub fn untyped(&self) -> SyntaxStablePtrId {
                self.0
            }
            pub fn lookup(&self, db: &dyn SyntaxGroup) -> $(&name) {
                $(&name)::from_syntax_node(db, self.0.lookup(db))
            }
        }
        #[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
        pub enum $(&element_or_separator_green_name) {
            Separator($(&separator_green_name)),
            Element($(&element_green_name)),
        }
        impl From<$(&separator_green_name)> for $(&element_or_separator_green_name) {
            fn from(value: $(&separator_green_name)) -> Self {
                $(&element_or_separator_green_name)::Separator(value)
            }
        }
        impl From<$(&element_green_name)> for $(&element_or_separator_green_name) {
            fn from(value: $(&element_green_name)) -> Self {
                $(&element_or_separator_green_name)::Element(value)
            }
        }
        impl $(&element_or_separator_green_name) {
            fn id(&self) -> GreenId {
                match self {
                    $(&element_or_separator_green_name)::Separator(green) => green.0,
                    $(&element_or_separator_green_name)::Element(green) => green.0,
                }
            }
        }
        $common_code
    }
}

fn gen_common_list_code(name: &str, green_name: &str, ptr_name: &str) -> rust::Tokens {
    quote! {
        #[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
        pub struct $green_name(pub GreenId);
        impl TypedSyntaxNode for $name {
            const OPTIONAL_KIND: Option<SyntaxKind> = Some(SyntaxKind::$name);
            type StablePtr = $ptr_name;
            type Green = $green_name;
            fn missing(db: &dyn SyntaxGroup) -> Self::Green {
                $green_name(db.intern_green(Arc::new(
                    GreenNode {
                        kind: SyntaxKind::$name,
                        details: GreenNodeDetails::Node { children: vec![], width: TextWidth::default() },
                    })
                ))
            }
            fn from_syntax_node(db: &dyn SyntaxGroup, node: SyntaxNode) -> Self {
                Self(ElementList::new(node))
            }
            fn as_syntax_node(&self) -> SyntaxNode{
                self.node.clone()
            }
            fn stable_ptr(&self) -> Self::StablePtr {
                $ptr_name(self.node.0.stable_ptr)
            }
        }
    }
}

fn gen_enum_code(
    name: String,
    variants: Vec<Variant>,
    missing_variant: Option<Variant>,
) -> rust::Tokens {
    let ptr_name = format!("{name}Ptr");
    let green_name = format!("{name}Green");
    let mut enum_body = quote! {};
    let mut from_node_body = quote! {};
    let mut ptr_conversions = quote! {};
    let mut green_conversions = quote! {};
    for variant in &variants {
        let n = &variant.name;
        let k = &variant.kind;

        enum_body.extend(quote! {
            $n($k),
        });
        from_node_body.extend(quote! {
            SyntaxKind::$k => $(&name)::$n($k::from_syntax_node(db, node)),
        });
        let variant_ptr = format!("{k}Ptr");
        ptr_conversions.extend(quote! {
            impl From<$(&variant_ptr)> for $(&ptr_name) {
                fn from(value: $(&variant_ptr)) -> Self {
                    Self(value.0)
                }
            }
        });
        let variant_green = format!("{k}Green");
        green_conversions.extend(quote! {
            impl From<$(&variant_green)> for $(&green_name) {
                fn from(value: $(&variant_green)) -> Self {
                    Self(value.0)
                }
            }
        });
    }
    let missing_body = match missing_variant {
        Some(missing) => quote! {
            $(&green_name)($(missing.kind)::missing(db).0)
        },
        None => quote! {
            panic!("No missing variant.");
        },
    };
    quote! {
        #[derive(Clone, Debug, Eq, Hash, PartialEq)]
        pub enum $(&name){
            $enum_body
        }
        #[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
        pub struct $(&ptr_name)(pub SyntaxStablePtrId);
        impl $(&ptr_name) {
            pub fn untyped(&self) -> SyntaxStablePtrId {
                self.0
            }
            pub fn lookup(&self, db: &dyn SyntaxGroup) -> $(&name) {
                $(&name)::from_syntax_node(db, self.0.lookup(db))
            }
        }
        $ptr_conversions
        $green_conversions
        #[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
        pub struct $(&green_name)(pub GreenId);
        impl TypedSyntaxNode for $(&name){
            const OPTIONAL_KIND: Option<SyntaxKind> = None;
            type StablePtr = $(&ptr_name);
            type Green = $(&green_name);
            fn missing(db: &dyn SyntaxGroup) -> Self::Green {
                $missing_body
            }
            fn from_syntax_node(db: &dyn SyntaxGroup, node: SyntaxNode) -> Self {
                let kind = node.kind(db);
                match kind{
                    $from_node_body
                    _ => panic!(
                        "Unexpected syntax kind {:?} when constructing {}.",
                        kind,
                        $[str]($[const](&name))),
                }
            }
            fn as_syntax_node(&self) -> SyntaxNode {
                match self {
                    $(for v in &variants => $(&name)::$(&v.name)(x) => x.as_syntax_node(),)
                }
            }
            fn stable_ptr(&self) -> Self::StablePtr {
                $(&ptr_name)(self.as_syntax_node().0.stable_ptr)
            }
        }
        impl $(&name){
            // Checks if a kind of a variant of $(&name).
            #[allow(clippy::match_like_matches_macro)]
            pub fn is_variant(kind: SyntaxKind) -> bool {
                match kind {
                    $(for v in &variants => SyntaxKind::$(&v.kind) => true,)
                    _ => false,
                }
            }
        }
    }
}

fn gen_token_code(name: String) -> rust::Tokens {
    let green_name = format!("{name}Green");
    let ptr_name = format!("{name}Ptr");

    quote! {
        #[derive(Clone, Debug, Eq, Hash, PartialEq)]
        pub struct $(&name) {
            node: SyntaxNode,
        }
        impl Token for $(&name) {
            fn new_green(db: &dyn SyntaxGroup, text: SmolStr) -> Self::Green {
                $(&green_name)(db.intern_green(Arc::new(GreenNode {
                    kind: SyntaxKind::$(&name),
                    details: GreenNodeDetails::Token(text),
                })))
            }
            fn text(&self, db: &dyn SyntaxGroup) -> SmolStr {
                extract_matches!(&db.lookup_intern_green(
                    self.node.0.green).details, GreenNodeDetails::Token).clone()
            }
        }
        #[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
        pub struct $(&ptr_name)(pub SyntaxStablePtrId);
        impl $(&ptr_name) {
            pub fn untyped(&self) -> SyntaxStablePtrId {
                self.0
            }
            pub fn lookup(&self, db: &dyn SyntaxGroup) -> $(&name) {
                $(&name)::from_syntax_node(db, self.0.lookup(db))
            }
        }
        #[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
        pub struct $(&green_name)(pub GreenId);
        impl $(&green_name) {
            pub fn text(&self, db: &dyn SyntaxGroup) -> SmolStr {
                extract_matches!(
                    &db.lookup_intern_green(self.0).details, GreenNodeDetails::Token).clone()
            }
        }
        impl TypedSyntaxNode for $(&name){
            const OPTIONAL_KIND: Option<SyntaxKind> = Some(SyntaxKind::$(&name));
            type StablePtr = $(&ptr_name);
            type Green = $(&green_name);
            fn missing(db: &dyn SyntaxGroup) -> Self::Green {
                $(&green_name)(db.intern_green(Arc::new(GreenNode {
                    kind: SyntaxKind::TokenMissing,
                    details: GreenNodeDetails::Token("".into()),
                })))
            }
            fn from_syntax_node(db: &dyn SyntaxGroup, node: SyntaxNode) -> Self {
                match db.lookup_intern_green(node.0.green).details {
                    GreenNodeDetails::Token(_) => Self { node },
                    GreenNodeDetails::Node { .. } => panic!(
                        "Expected a token {:?}, not an internal node",
                        SyntaxKind::$(&name)
                    ),
                }
            }
            fn as_syntax_node(&self) -> SyntaxNode {
                self.node.clone()
            }
            fn stable_ptr(&self) -> Self::StablePtr {
                $(&ptr_name)(self.node.0.stable_ptr)
            }
        }
    }
}

fn gen_struct_code(name: String, members: Vec<Member>, is_terminal: bool) -> rust::Tokens {
    let green_name = format!("{name}Green");
    let mut body = rust::Tokens::new();
    let mut field_indices = quote! {};
    let mut args = quote! {};
    let mut params = quote! {};
    let mut args_for_missing = quote! {};
    let mut ptr_getters = quote! {};
    let mut key_field_index: usize = 0;
    for (i, Member { name, kind, key }) in members.iter().enumerate() {
        let index_name = format!("INDEX_{}", name.to_uppercase());
        field_indices.extend(quote! {
            pub const $index_name : usize = $i;
        });
        let key_name_green = format!("{name}_green");
        args.extend(quote! {$name.0,});
        // TODO(spapini): Validate that children SyntaxKinds are as expected.

        let child_green = format!("{kind}Green");
        params.extend(quote! {$name: $(&child_green),});
        body.extend(quote! {
            pub fn $name(&self, db: &dyn SyntaxGroup) -> $kind {
                $kind::from_syntax_node(db, self.children[$i].clone())
            }
        });
        args_for_missing.extend(quote! {$kind::missing(db).0,});

        if *key {
            ptr_getters.extend(quote! {
                pub fn $(&key_name_green)(self, db: &dyn SyntaxGroup) -> $(&child_green) {
                    let ptr = db.lookup_intern_stable_ptr(self.0);
                    if let SyntaxStablePtr::Child { key_fields, .. } = ptr {
                        $(&child_green)(key_fields[$key_field_index])
                    } else {
                        panic!("Unexpected key field query on root.");
                    }
                }
            });
            key_field_index += 1;
        }
    }
    let ptr_name = format!("{name}Ptr");
    let new_green_impl = if is_terminal {
        let token_name = name.replace("Terminal", "Token");
        quote! {
            impl Terminal for $(&name) {
                const KIND: SyntaxKind = SyntaxKind::$(&name);
                type TokenType = $(&token_name);
                fn new_green(
                    db: &dyn SyntaxGroup,
                    leading_trivia: TriviaGreen,
                    token: <<$(&name) as Terminal>::TokenType as TypedSyntaxNode>::Green,
                    trailing_trivia: TriviaGreen
                ) -> Self::Green {
                    let children: Vec<GreenId> = vec![$args];
                    let width = children.iter().copied().map(|id|
                        db.lookup_intern_green(id).width()).sum();
                    $(&green_name)(db.intern_green(Arc::new(GreenNode {
                        kind: SyntaxKind::$(&name),
                        details: GreenNodeDetails::Node { children, width },
                    })))
                }
                fn text(&self, db: &dyn SyntaxGroup) -> SmolStr {
                    self.token(db).text(db)
                }
            }
        }
    } else {
        quote! {
            impl $(&name) {
                $field_indices
                pub fn new_green(db: &dyn SyntaxGroup, $params) -> $(&green_name) {
                    let children: Vec<GreenId> = vec![$args];
                    let width = children.iter().copied().map(|id|
                        db.lookup_intern_green(id).width()).sum();
                    $(&green_name)(db.intern_green(Arc::new(GreenNode {
                        kind: SyntaxKind::$(&name),
                        details: GreenNodeDetails::Node { children, width },
                    })))
                }
            }
        }
    };
    quote! {
        #[derive(Clone, Debug, Eq, Hash, PartialEq)]
        pub struct $(&name){
            node: SyntaxNode,
            children: Arc<Vec<SyntaxNode>>,
        }
        $new_green_impl
        impl $(&name) {
            $body
        }
        #[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
        pub struct $(&ptr_name)(pub SyntaxStablePtrId);
        impl $(&ptr_name) {
            $ptr_getters

            pub fn untyped(&self) -> SyntaxStablePtrId {
                self.0
            }
            pub fn lookup(&self, db: &dyn SyntaxGroup) -> $(&name) {
                $(&name)::from_syntax_node(db, self.0.lookup(db))
            }
        }
        #[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
        pub struct $(&green_name)(pub GreenId);
        impl TypedSyntaxNode for $(&name){
            const OPTIONAL_KIND: Option<SyntaxKind> = Some(SyntaxKind::$(&name));
            type StablePtr = $(&ptr_name);
            type Green = $(&green_name);
            fn missing(db: &dyn SyntaxGroup) -> Self::Green {
                // Note: A missing syntax element should result in an internal green node
                // of width 0, with as much structure as possible.
                $(&green_name)(db.intern_green(Arc::new(GreenNode {
                    kind: SyntaxKind::$(&name),
                    details: GreenNodeDetails::Node {
                        children: vec![$args_for_missing],
                        width: TextWidth::default(),
                    },
                })))
            }
            fn from_syntax_node(db: &dyn SyntaxGroup, node: SyntaxNode) -> Self {
                let kind = node.kind(db);
                assert_eq!(kind, SyntaxKind::$(&name), "Unexpected SyntaxKind {:?}. Expected {:?}.", kind, SyntaxKind::$(&name));
                let children = db.get_children(node.clone());
                Self { node, children }
            }
            fn as_syntax_node(&self) -> SyntaxNode {
                self.node.clone()
            }
            fn stable_ptr(&self) -> Self::StablePtr {
                $(&ptr_name)(self.node.0.stable_ptr)
            }
        }
    }
}

/// Appends the given rust token to the given list
fn append_rust_token(list: &mut rust::Tokens, name: &str) {
    if list.is_empty() {
        list.append(format!("SyntaxKind::{name}"));
    } else {
        list.append(format!("| SyntaxKind::{name}"));
    }
}
