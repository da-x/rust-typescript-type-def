use crate::type_expr::{
    DefinedTypeInfo, Docs, Ident, NativeTypeInfo, ObjectField, TypeArray,
    TypeDefinition, TypeExpr, TypeInfo, TypeIntersection, TypeName, TypeObject,
    TypeString, TypeTuple, TypeUnion,
};
use std::io;

/// A Rust type that has a corresponding TypeScript type definition.
///
/// For a Rust type `T`, the `TypeDef` trait defines a TypeScript type
/// which describes JavaScript value that are equivalents of Rust values of
/// type `T` as encoded to JSON using [`serde_json`](https://docs.rs/serde_json/). The
/// types are one-to-one, so decoding from TypeScript to JSON to Rust also
/// works.
///
/// ## Implementing
///
/// ### Local Types
///
/// To derive this trait for your own types, use the
/// [`#[derive(TypeDef)]`](macro@crate::TypeDef) macro.
///
/// ### Foreign Types
///
/// To use types from external crates in your own types, the recommended
/// approach is to create a newtype wrapper and use the `#[type_def(type_of =
/// "T")]` attribute to specify its type:
///
/// ```
/// use serde::{Deserialize, Serialize};
/// use typescript_type_def::{write_definition_file, TypeDef};
///
/// // The Uuid type from the uuid crate does not implement TypeDef
/// // But we know that it serializes to just a string
/// #[derive(
///     Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TypeDef,
/// )]
/// #[serde(transparent)]
/// pub struct Uuid(#[type_def(type_of = "String")] pub uuid::Uuid);
///
/// // We can now use our newtype in place of the foreign type
/// #[derive(Debug, Serialize, Deserialize, TypeDef)]
/// pub struct User {
///     pub id: Uuid,
///     pub name: String,
/// }
///
/// let ts_module = {
///     let mut buf = Vec::new();
///     write_definition_file::<_, User>(&mut buf, Default::default()).unwrap();
///     String::from_utf8(buf).unwrap()
/// };
/// assert_eq!(
///     ts_module,
///     r#"// AUTO-GENERATED by typescript-type-def
///
/// export default types;
/// export namespace types{
/// export type Uuid=string;
/// export type User={"id":types.Uuid;"name":string;};
/// }
/// "#
/// );
/// ```
///
/// The other option if you don't want to create a newtype is to use
/// `#[type_def(type_of = "T")]` everywhere you use the type:
///
/// ```
/// use serde::{Deserialize, Serialize};
/// use typescript_type_def::{write_definition_file, TypeDef};
///
/// #[derive(Debug, Serialize, Deserialize, TypeDef)]
/// pub struct User {
///     #[type_def(type_of = "String")]
///     pub id: uuid::Uuid,
///     pub name: String,
/// }
///
/// let ts_module = {
///     let mut buf = Vec::new();
///     write_definition_file::<_, User>(&mut buf, Default::default()).unwrap();
///     String::from_utf8(buf).unwrap()
/// };
/// assert_eq!(
///     ts_module,
///     r#"// AUTO-GENERATED by typescript-type-def
///
/// export default types;
/// export namespace types{
/// export type User={"id":string;"name":string;};
/// }
/// "#
/// );
/// ```
///
/// ### [`std`] Types
///
/// [`TypeDef`] is implemented for [`std`] types as follows:
///
/// | Rust type | TypeScript type |
/// |---|---|
/// | [`bool`] | `boolean` |
/// | [`String`] | `string` |
/// | [`str`] | `string` |
/// | numeric types | `number`[^number] |
/// | [`()`](unit) | `null` |
/// | [`(A, B, C)`](tuple) | `[A, B, C]` |
/// | [`[T; N]`](array) | `[T, T, ..., T]` (an `N`-tuple) |
// FIXME: https://github.com/rust-lang/rust/issues/86375
/// | [`Option<T>`] | <code>T \| null</code> |
/// | [`Vec<T>`] | `T[]` |
/// | [`[T]`](slice) | `T[]` |
/// | [`HashSet<T>`](std::collections::HashSet) | `T[]` |
/// | [`BTreeSet<T>`](std::collections::BTreeSet) | `T[]` |
/// | [`HashMap<K, V>`](std::collections::HashMap) | `Record<K, V>` |
/// | [`BTreeMap<K, V>`](std::collections::BTreeMap) | `Record<K, V>` |
/// | [`&'static T`](reference) | `T` |
/// | [`Box<T>`] | `T` |
/// | [`Cow<'static, T>`](std::borrow::Cow) | `T` |
/// | [`PhantomData<T>`](std::marker::PhantomData) | `T` |
///
/// [^number]: Numeric types are emitted as named aliases converted to
/// PascalCase (e.g. `Usize`, `I32`, `F64`, `NonZeroI8`, etc.). Since they are
/// simple aliases they do not enforce anything in TypeScript about the Rust
/// types' numeric bounds, but serve to document their intended range.
pub trait TypeDef: 'static {
    /// A constant value describing the structure of this type.
    ///
    /// This type information is used to emit a TypeScript type definition.
    const INFO: TypeInfo;
}

pub(crate) struct EmitCtx<'ctx> {
    w: &'ctx mut dyn io::Write,
    options: DefinitionFileOptions<'ctx>,
    stats: Stats,
}

pub(crate) trait Emit {
    fn emit(&self, ctx: &mut EmitCtx<'_>) -> io::Result<()>;
}

/// Options for customizing the output of [`write_definition_file`].
///
/// The default options are:
/// ```
/// # use typescript_type_def::DefinitionFileOptions;
/// # let default =
/// DefinitionFileOptions {
///     header: Some("// AUTO-GENERATED by typescript-type-def\n"),
///     root_namespace: Some("types"),
/// }
/// # ;
/// # assert_eq!(default, Default::default());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefinitionFileOptions<'a> {
    /// Text to be emitted at the start of the file.
    ///
    /// If `Some`, the string should contain the exact content of the header as
    /// TypeScript code (usually in the form of comments). If `None`, no header
    /// will be added.
    pub header: Option<&'a str>,
    /// The name of the root namespace which the definitions will be placed
    /// under.
    ///
    /// The reason all definitions must be placed under a root namespace is to
    /// prevent name ambiguities. Consider the following TypeScript module:
    /// ```typescript
    /// type Foo = number;
    /// export namespace foo {
    ///     type Foo = string;
    ///     type Bar = { x: Foo };
    /// }
    /// ```
    /// In this case, the type that `Bar.x` refers to is ambiguous; it could be
    /// either the top-level `Foo` or the adjacent `Foo`. Placing all types
    /// under a namespace and referencing them by full path removes this
    /// ambiguity:
    /// ```typescript
    /// export namespace root {
    ///     type Foo = number;
    ///     export namespace foo {
    ///         type Foo = string;
    ///         type Bar = { x: root.Foo };
    ///     }
    /// }
    /// ```
    ///
    /// By default, all exported types are wrapped in a root namespace `types`.
    /// This gives all types an unambiguous fully-qualified name. When setting
    /// the `root_namespace` to `None`, no outer namespace is added. This will
    /// work fine in many situations, it can however lead to errors in the generated
    /// TypeScript code also using inner namespaces. When using inner namespaces
    /// through the `#[type_def(namespace = "x.y.z")]` attribute, you want to
    /// keep the `root_namespace` as well.
    pub root_namespace: Option<&'a str>,
}

/// Statistics about the type definitions produced by [`write_definition_file`].
#[derive(Debug, Clone)]
pub struct Stats {
    /// The number of unique type definitions produced.
    pub type_definitions: usize,
}

impl<'ctx> EmitCtx<'ctx> {
    fn new(
        w: &'ctx mut dyn io::Write,
        options: DefinitionFileOptions<'ctx>,
    ) -> Self {
        let stats = Stats {
            type_definitions: 0,
        };
        Self { w, options, stats }
    }
}

struct SepList<'a, T>(&'a [T], &'static str);

impl<'a, T> Emit for SepList<'a, T>
where
    T: Emit,
{
    fn emit(&self, ctx: &mut EmitCtx<'_>) -> io::Result<()> {
        let Self(elements, separator) = self;
        let mut first = true;
        for element in *elements {
            if !first {
                write!(ctx.w, "{}", separator)?;
            }
            element.emit(ctx)?;
            first = false;
        }
        Ok(())
    }
}

struct Generics<'a, T>(&'a [T]);

impl<'a, T> Emit for Generics<'a, T>
where
    T: Emit,
{
    fn emit(&self, ctx: &mut EmitCtx<'_>) -> io::Result<()> {
        let Self(args) = self;
        if !args.is_empty() {
            write!(ctx.w, "<")?;
            SepList(args, ",").emit(ctx)?;
            write!(ctx.w, ">")?;
        }
        Ok(())
    }
}

impl Emit for TypeExpr {
    fn emit(&self, ctx: &mut EmitCtx<'_>) -> io::Result<()> {
        match self {
            TypeExpr::Ref(TypeInfo::Native(NativeTypeInfo { r#ref })) => {
                r#ref.emit(ctx)
            }
            TypeExpr::Ref(TypeInfo::Defined(DefinedTypeInfo {
                def:
                    TypeDefinition {
                        docs: _,
                        path,
                        name,
                        generic_vars: _,
                        def: _,
                    },
                generic_args,
            })) => {
                if let Some(root_namespace) = ctx.options.root_namespace {
                    write!(ctx.w, "{}.", root_namespace)?;
                }
                for path_part in *path {
                    path_part.emit(ctx)?;
                    write!(ctx.w, ".")?;
                }
                name.emit(ctx)?;
                Generics(generic_args).emit(ctx)?;
                Ok(())
            }
            TypeExpr::Name(type_name) => type_name.emit(ctx),
            TypeExpr::String(type_string) => type_string.emit(ctx),
            TypeExpr::Tuple(type_tuple) => type_tuple.emit(ctx),
            TypeExpr::Object(type_object) => type_object.emit(ctx),
            TypeExpr::Array(type_array) => type_array.emit(ctx),
            TypeExpr::Union(type_union) => type_union.emit(ctx),
            TypeExpr::Intersection(type_intersection) => {
                type_intersection.emit(ctx)
            }
        }
    }
}

impl Emit for TypeName {
    fn emit(&self, ctx: &mut EmitCtx<'_>) -> io::Result<()> {
        let Self {
            path,
            name,
            generic_args,
        } = self;
        for path_part in *path {
            path_part.emit(ctx)?;
            write!(ctx.w, ".")?;
        }
        name.emit(ctx)?;
        Generics(generic_args).emit(ctx)?;
        Ok(())
    }
}

impl Emit for TypeString {
    fn emit(&self, ctx: &mut EmitCtx<'_>) -> io::Result<()> {
        let Self { docs, value } = self;
        docs.emit(ctx)?;
        write!(ctx.w, "{:?}", value)?;
        Ok(())
    }
}

impl Emit for TypeTuple {
    fn emit(&self, ctx: &mut EmitCtx<'_>) -> io::Result<()> {
        let Self { docs, elements } = self;
        docs.emit(ctx)?;
        write!(ctx.w, "[")?;
        SepList(elements, ",").emit(ctx)?;
        write!(ctx.w, "]")?;
        Ok(())
    }
}

impl Emit for TypeObject {
    fn emit(&self, ctx: &mut EmitCtx<'_>) -> io::Result<()> {
        let Self { docs, fields } = self;
        docs.emit(ctx)?;
        write!(ctx.w, "{{")?;
        for ObjectField {
            docs,
            name,
            optional,
            r#type,
        } in *fields
        {
            docs.emit(ctx)?;
            name.emit(ctx)?;
            if *optional {
                write!(ctx.w, "?")?;
            }
            write!(ctx.w, ":")?;
            r#type.emit(ctx)?;
            write!(ctx.w, ";")?;
        }
        write!(ctx.w, "}}")?;
        Ok(())
    }
}

impl Emit for TypeArray {
    fn emit(&self, ctx: &mut EmitCtx<'_>) -> io::Result<()> {
        let Self { docs, item } = self;
        docs.emit(ctx)?;
        write!(ctx.w, "(")?;
        item.emit(ctx)?;
        write!(ctx.w, ")[]")?;
        Ok(())
    }
}

impl Emit for TypeUnion {
    fn emit(&self, ctx: &mut EmitCtx<'_>) -> io::Result<()> {
        let Self { docs, members } = self;
        docs.emit(ctx)?;
        if members.is_empty() {
            write!(ctx.w, "never")?;
        } else {
            write!(ctx.w, "(")?;
            SepList(members, "|").emit(ctx)?;
            write!(ctx.w, ")")?;
        }
        Ok(())
    }
}

impl Emit for TypeIntersection {
    fn emit(&self, ctx: &mut EmitCtx<'_>) -> io::Result<()> {
        let Self { docs, members } = self;
        docs.emit(ctx)?;
        if members.is_empty() {
            write!(ctx.w, "any")?;
        } else {
            write!(ctx.w, "(")?;
            SepList(members, "&").emit(ctx)?;
            write!(ctx.w, ")")?;
        }
        Ok(())
    }
}

impl Emit for Ident {
    fn emit(&self, ctx: &mut EmitCtx<'_>) -> io::Result<()> {
        let Self(name) = self;
        write!(ctx.w, "{}", name)?;
        Ok(())
    }
}

impl Emit for Docs {
    fn emit(&self, ctx: &mut EmitCtx<'_>) -> io::Result<()> {
        let Self(docs) = self;
        writeln!(ctx.w)?;
        writeln!(ctx.w, "/**")?;
        for line in docs.lines() {
            writeln!(ctx.w, " * {}", line)?;
        }
        writeln!(ctx.w, " */")?;
        Ok(())
    }
}

impl<T> Emit for &T
where
    T: Emit,
{
    fn emit(&self, ctx: &mut EmitCtx<'_>) -> io::Result<()> {
        T::emit(self, ctx)
    }
}

impl<T> Emit for Option<T>
where
    T: Emit,
{
    fn emit(&self, ctx: &mut EmitCtx<'_>) -> io::Result<()> {
        if let Some(inner) = self {
            inner.emit(ctx)
        } else {
            Ok(())
        }
    }
}

impl EmitCtx<'_> {
    fn emit_type(&mut self, info: &'static TypeInfo) -> io::Result<()> {
        for TypeDefinition {
            docs,
            path,
            name,
            generic_vars,
            def,
        } in crate::iter_def_deps::IterDefDeps::new(info)
        {
            self.stats.type_definitions += 1;
            docs.emit(self)?;
            if !path.is_empty() {
                write!(self.w, "export namespace ")?;
                SepList(path, ".").emit(self)?;
                write!(self.w, "{{")?;
            }
            write!(self.w, "export type ")?;
            name.emit(self)?;
            Generics(generic_vars).emit(self)?;
            write!(self.w, "=")?;
            def.emit(self)?;
            write!(self.w, ";")?;
            if !path.is_empty() {
                write!(self.w, "}}")?;
            }
            writeln!(self.w)?;
        }
        Ok(())
    }
}

impl Default for DefinitionFileOptions<'_> {
    fn default() -> Self {
        Self {
            header: Some("// AUTO-GENERATED by typescript-type-def\n"),
            root_namespace: Some("types"),
        }
    }
}

/// Writes a TypeScript definition file containing type definitions for `T` to
/// the writer `W`.
///
/// The resulting TypeScript module will define and export the type definition
/// for `T` and all of its transitive dependencies under a root namespace. The
/// name of the root namespace is configurable with
/// [`options`](DefinitionFileOptions::root_namespace). Each type definition may
/// additionally have its own nested namespace under the root namespace. The
/// root namespace will also be the default export of the module.
///
/// If the root namespace is set to `None`, no root namespace and no default
/// export will be added. When not using a root namespace, do not use nested
/// namespaces either, as that can lead to unresolved type references.
///
/// The file will also include a header comment indicating that it was
/// auto-generated by this library. This is configurable with
/// [`options`](DefinitionFileOptions::header).
///
/// Note that the TypeScript code generated by this library is not very
/// human-readable. To make the code human-readable, use a TypeScript code
/// formatter (such as [Prettier](https://prettier.io/)) on the output.
pub fn write_definition_file<W, T: ?Sized>(
    mut writer: W,
    options: DefinitionFileOptions<'_>,
) -> io::Result<Stats>
where
    W: io::Write,
    T: TypeDef,
{
    let mut ctx = EmitCtx::new(&mut writer, options);
    if let Some(header) = &ctx.options.header {
        writeln!(ctx.w, "{}", header)?;
    }
    if let Some(root_namespace) = options.root_namespace {
        writeln!(ctx.w, "export default {};", root_namespace)?;
        writeln!(ctx.w, "export namespace {}{{", root_namespace)?;
    }
    ctx.emit_type(&T::INFO)?;
    if options.root_namespace.is_some() {
        writeln!(ctx.w, "}}")?;
    }
    Ok(ctx.stats)
}
