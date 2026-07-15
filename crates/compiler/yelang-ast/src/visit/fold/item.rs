/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2025 Oyelowo Oyedayo
 */

use crate::{
    common::{self, *},
    expr::{self, *},
    item::{self, *},
    pattern::{self, *},
    ptr::{self, *},
    query::{self, *},
    stmt::{self, *},
    tokenizer::{self, *},
    types::{self, *},
    visit::fold::folder::Folder,
};

use crate::item::{Item, ItemKind, StructFields};
use crate::types::{TypeKind, TypeOperator};

pub fn fold_item<F: Folder + ?Sized>(f: &mut F, node: Item) -> Item {
    let kind = match node.kind {
        ItemKind::Fn(func) => ItemKind::Fn(Box::new(f.fold_fn(*func))),
        ItemKind::Struct(s) => ItemKind::Struct(f.fold_struct(s)),
        ItemKind::Enum(e) => ItemKind::Enum(f.fold_enum(e)),
        ItemKind::TypeAlias(t) => ItemKind::TypeAlias(Box::new(f.fold_type_alias(*t))),
        ItemKind::Trait(t) => ItemKind::Trait(Box::new(f.fold_trait(*t))),
        ItemKind::Const(c) => ItemKind::Const(Box::new(f.fold_const(*c))),
        ItemKind::Static(s) => ItemKind::Static(Box::new(f.fold_static(*s))),
        ItemKind::Impl(i) => ItemKind::Impl(Box::new(f.fold_impl(*i))),
        ItemKind::Use(u) => ItemKind::Use(f.fold_use(u)),
        ItemKind::Module(m) => ItemKind::Module(f.fold_module(m)),
        ItemKind::MacroDef(def) => ItemKind::MacroDef(def),
    };

    Item {
        kind,
        attributes: node
            .attributes
            .into_iter()
            .map(|a| f.fold_attribute(a))
            .collect(),
        visibility: node.visibility,
        span: node.span,
    }
}

pub fn fold_fn<F: Folder + ?Sized>(f: &mut F, func: item::FnDef) -> item::FnDef {
    item::FnDef {
        name: func.name,
        is_const: func.is_const,
        generics: f.fold_generics(func.generics),
        sig: f.fold_fn_sig(func.sig),
        body: f.fold_block_expr(func.body),
        span: func.span,
    }
}

pub fn fold_struct<F: Folder + ?Sized>(f: &mut F, s: item::Struct) -> item::Struct {
    item::Struct {
        name: s.name,
        generics: f.fold_generics(s.generics),
        fields: match s.fields {
            StructFields::Named(fields) => StructFields::Named(
                fields
                    .into_iter()
                    .map(|field| f.fold_field_def(field))
                    .collect(),
            ),
            StructFields::Tuple(tys) => {
                StructFields::Tuple(tys.into_iter().map(|t| f.fold_type(t)).collect())
            }
            StructFields::Unit => StructFields::Unit,
        },
        span: s.span,
    }
}

pub fn fold_enum<F: Folder + ?Sized>(f: &mut F, e: item::Enum) -> item::Enum {
    item::Enum {
        name: e.name,
        generics: f.fold_generics(e.generics),
        variants: e
            .variants
            .into_iter()
            .map(|v| f.fold_variant_def(v))
            .collect(),
    }
}

pub fn fold_trait<F: Folder + ?Sized>(f: &mut F, t: item::Trait) -> item::Trait {
    item::Trait {
        name: t.name,
        generics: f.fold_generics(t.generics),
        super_traits: t.super_traits,
        items: t
            .items
            .into_iter()
            .map(|item| f.fold_trait_item(item))
            .collect(),
        span: t.span,
    }
}

pub fn fold_impl<F: Folder + ?Sized>(f: &mut F, i: item::Impl) -> item::Impl {
    item::Impl {
        attributes: i.attributes,
        defaultness: i.defaultness,
        // visibility: i.visibility,
        generics: f.fold_generics(i.generics),
        trait_impl: i.trait_impl.map(|path| f.fold_path(path)),
        is_negative: i.is_negative,
        self_ty: f.fold_type(i.self_ty),
        items: i
            .items
            .into_iter()
            .map(|item| f.fold_impl_item(item))
            .collect(),
        span: i.span,
    }
}

pub fn fold_module<F: Folder + ?Sized>(f: &mut F, m: item::ModDef) -> item::ModDef {
    item::ModDef {
        name: m.name,
        kind: match m.kind {
            item::ModKind::Inline { items } => item::ModKind::Inline {
                items: items.into_iter().map(|item| f.fold_item(item)).collect(),
            },
            item::ModKind::External => item::ModKind::External,
        },
    }
}

pub fn fold_type_alias<F: Folder + ?Sized>(f: &mut F, t: item::TypeAlias) -> item::TypeAlias {
    item::TypeAlias {
        name: t.name,
        generics: f.fold_generics(t.generics),
        target: f.fold_type(t.target),
        span: t.span,
    }
}

pub fn fold_const<F: Folder + ?Sized>(f: &mut F, c: item::Const) -> item::Const {
    item::Const {
        name: c.name,
        ty: f.fold_type(c.ty),
        value: f.fold_expr(c.value),
    }
}

pub fn fold_static<F: Folder + ?Sized>(f: &mut F, s: item::Static) -> item::Static {
    item::Static {
        name: s.name,
        ty: f.fold_type(s.ty),
        value: f.fold_expr(s.value),
        mutability: s.mutability,
    }
}

pub fn fold_type<F: Folder + ?Sized>(f: &mut F, ty: Type) -> Type {
    let kind = match ty.kind {
        TypeKind::Array(t, len) => {
            TypeKind::Array(Box::new(f.fold_type(*t)), Box::new(f.fold_expr(*len)))
        }
        TypeKind::Tuple(tys) => TypeKind::Tuple(tys.into_iter().map(|t| f.fold_type(t)).collect()),
        TypeKind::Slice(t) => TypeKind::Slice(Box::new(f.fold_type(*t))),
        TypeKind::Ref { ty, is_mut } => TypeKind::Ref {
            ty: Box::new(f.fold_type(*ty)),
            is_mut,
        },
        TypeKind::Named(path) => TypeKind::Named(f.fold_path(path)),
        TypeKind::Function(func_ty) => TypeKind::Function(crate::FunctionType {
            is_async: func_ty.is_async,
            params: func_ty.params.into_iter().map(|p| f.fold_type(p)).collect(),
            return_type: Box::new(f.fold_type(*func_ty.return_type)),
            is_variadic: func_ty.is_variadic,
        }),
        TypeKind::ForAll { params, ty } => TypeKind::ForAll {
            params: item::TypeBinderParams {
                params: params
                    .params
                    .into_iter()
                    .map(|p| match p {
                        item::TypeBinderParam::Type(tp) => {
                            item::TypeBinderParam::Type(item::TypeBinderTyParam {
                                name: f.fold_ident(tp.name),
                                bounds: tp
                                    .bounds
                                    .into_iter()
                                    .map(|b| f.fold_trait_bound(b))
                                    .collect(),
                                span: tp.span,
                            })
                        }
                        item::TypeBinderParam::Const(c) => {
                            item::TypeBinderParam::Const(item::ConstBinderParam {
                                name: f.fold_ident(c.name),
                                ty: f.fold_type(c.ty),
                                span: c.span,
                            })
                        }
                    })
                    .collect(),
                span: params.span,
            },
            ty: Box::new(f.fold_type(*ty)),
        },
        TypeKind::Structural(fields) => TypeKind::Structural(
            fields
                .into_iter()
                .map(|field| crate::StructuralField {
                    name: field.name,
                    ty: f.fold_type(field.ty),
                    optional: field.optional,
                })
                .collect(),
        ),
        TypeKind::Never => TypeKind::Never,
        TypeKind::Infer => TypeKind::Infer,
        TypeKind::Union(types) => {
            TypeKind::Union(types.into_iter().map(|t| f.fold_type(t)).collect())
        }
        TypeKind::Literal(lit) => TypeKind::Literal(lit),
        TypeKind::Operator(op) => TypeKind::Operator(match op {
            TypeOperator::TypeOf(expr) => TypeOperator::TypeOf(Box::new(f.fold_expr(*expr))),
            TypeOperator::ReturnType(inner_ty) => {
                TypeOperator::ReturnType(Box::new(f.fold_type(*inner_ty)))
            }
            TypeOperator::Parameters(inner_ty) => {
                TypeOperator::Parameters(Box::new(f.fold_type(*inner_ty)))
            }
            TypeOperator::Pick(base, keys) => {
                TypeOperator::Pick(Box::new(f.fold_type(*base)), Box::new(f.fold_type(*keys)))
            }
            TypeOperator::Omit(base, keys) => {
                TypeOperator::Omit(Box::new(f.fold_type(*base)), Box::new(f.fold_type(*keys)))
            }
        }),
        TypeKind::ImplTrait(path) => TypeKind::ImplTrait(f.fold_path(path)),
        TypeKind::DynTrait(path) => TypeKind::DynTrait(f.fold_path(path)),
        TypeKind::Error => TypeKind::Error,
    };

    Type {
        kind,
        span: ty.span,
    }
}

pub fn fold_pattern<F: Folder + ?Sized>(f: &mut F, pat: Pattern) -> Pattern {
    let pattern = match pat.pattern {
        PatternKind::Binding {
            name,
            mutability,
            subpattern,
        } => PatternKind::Binding {
            name,
            mutability,
            subpattern: subpattern.map(|pat| Box::new(f.fold_pattern(*pat))),
        },
        PatternKind::Tuple { patterns } => PatternKind::Tuple {
            patterns: patterns.into_iter().map(|p| f.fold_pattern(p)).collect(),
        },
        PatternKind::Slice { patterns } => PatternKind::Slice {
            patterns: patterns.into_iter().map(|p| f.fold_pattern(p)).collect(),
        },
        PatternKind::Ref { pattern, is_mut } => PatternKind::Ref {
            pattern: Box::new(f.fold_pattern(*pattern)),
            is_mut,
        },
        PatternKind::Or(patterns) => {
            PatternKind::Or(patterns.into_iter().map(|p| f.fold_pattern(p)).collect())
        }
        PatternKind::Struct { path, fields, rest } => PatternKind::Struct {
            path: f.fold_path(path),
            fields: fields
                .into_iter()
                .map(|field| f.fold_field_pattern(field))
                .collect(),
            rest,
        },
        PatternKind::Record { fields, rest } => PatternKind::Record {
            fields: fields
                .into_iter()
                .map(|field| f.fold_field_pattern(field))
                .collect(),
            rest,
        },
        PatternKind::Absent => PatternKind::Absent,
        PatternKind::Wildcard => PatternKind::Wildcard,
        PatternKind::Path(path) => PatternKind::Path(f.fold_path(path)),
        PatternKind::Literal(lit) => PatternKind::Literal(lit),
        PatternKind::TupleStruct { path, patterns } => PatternKind::TupleStruct {
            path: f.fold_path(path),
            patterns: patterns.into_iter().map(|p| f.fold_pattern(p)).collect(),
        },
        PatternKind::Rest { name } => PatternKind::Rest { name },
        PatternKind::Range(range) => PatternKind::Range(crate::RangeExpr {
            start: range.start.map(|expr| Box::new(f.fold_expr(*expr))),
            op: range.op,
            end: range.end.map(|expr| Box::new(f.fold_expr(*expr))),
        }),
        PatternKind::Grouped(pat) => PatternKind::Grouped(Box::new(f.fold_pattern(*pat))),
    };

    Pattern {
        pattern,
        span: pat.span,
    }
}

pub fn fold_field_pattern<F: Folder + ?Sized>(f: &mut F, field: FieldPattern) -> FieldPattern {
    FieldPattern {
        name: field.name,
        pattern: f.fold_pattern(field.pattern),
        is_shorthand: field.is_shorthand,
        is_placeholder: field.is_placeholder,
    }
}

pub fn fold_variant_def<F: Folder + ?Sized>(f: &mut F, v: item::VariantDef) -> item::VariantDef {
    item::VariantDef {
        attributes: v
            .attributes
            .into_iter()
            .map(|a| f.fold_attribute(a))
            .collect(),
        name: v.name,
        kind: match v.kind {
            item::VariantKind::Struct(fields) => item::VariantKind::Struct(
                fields
                    .into_iter()
                    .map(|field| f.fold_field_def(field))
                    .collect(),
            ),
            item::VariantKind::Tuple(tys) => {
                item::VariantKind::Tuple(tys.into_iter().map(|t| f.fold_type(t)).collect())
            }
            item::VariantKind::Unit => item::VariantKind::Unit,
        },
        discriminant: v.discriminant.map(|e| f.fold_expr(e)),
        span: v.span,
    }
}

pub fn fold_use_tree<F: Folder + ?Sized>(f: &mut F, u: item::UseTree) -> item::UseTree {
    match u {
        item::UseTree::Simple { path, span } => item::UseTree::Simple {
            path: f.fold_path(path),
            span,
        },
        item::UseTree::Rename { path, alias, span } => item::UseTree::Rename {
            path: f.fold_path(path),
            alias,
            span,
        },
        item::UseTree::Glob { path, span } => item::UseTree::Glob {
            path: f.fold_path(path),
            span,
        },
        item::UseTree::Nested {
            prefix,
            items,
            span,
        } => item::UseTree::Nested {
            prefix: f.fold_path(prefix),
            items: items
                .into_iter()
                .map(|item| f.fold_use_tree(item))
                .collect(),
            span,
        },
    }
}

pub fn fold_trait_item_node<F: Folder + ?Sized>(
    f: &mut F,
    item: item::TraitItem,
) -> item::TraitItem {
    let item_kind = match item.item {
        item::TraitItemKind::Method(m) => item::TraitItemKind::Method(item::Method {
            segment: m.segment,
            is_const: m.is_const,
            generics: f.fold_generics(m.generics),
            sig: f.fold_fn_sig(m.sig),
            body: m.body.map(|b| f.fold_block_expr(b)),
        }),
        item::TraitItemKind::AssociatedType(t) => {
            item::TraitItemKind::AssociatedType(item::AssociatedType {
                name: t.name,
                generics: f.fold_generics(t.generics),
                bounds: t.bounds,
                default: t.default.map(|ty| f.fold_type(ty)),
                span: t.span,
            })
        }
        item::TraitItemKind::Constant(c) => item::TraitItemKind::Constant(item::AssociatedConst {
            name: c.name,
            ty: f.fold_type(c.ty),
            value: c.value.map(|v| f.fold_expr(v)),
            span: c.span,
        }),
    };
    item::TraitItem {
        item: item_kind,
        attributes: item.attributes,
        span: item.span,
    }
}

pub fn fold_impl_item_node<F: Folder + ?Sized>(f: &mut F, item: item::ImplItem) -> item::ImplItem {
    let item_kind = match item.item {
        item::ImplItemKind::Method(m) => item::ImplItemKind::Method(item::FnDef {
            name: m.name,
            is_const: m.is_const,
            generics: f.fold_generics(m.generics),
            sig: f.fold_fn_sig(m.sig),
            body: f.fold_block_expr(m.body),
            span: m.span,
        }),
        item::ImplItemKind::AssociatedType(t) => {
            item::ImplItemKind::AssociatedType(item::AssociatedTypeBinding {
                name: t.name,
                generics: f.fold_generics(t.generics),
                ty: f.fold_type(t.ty),
                span: t.span,
            })
        }
        item::ImplItemKind::Constant(c) => item::ImplItemKind::Constant(item::AssociatedConst {
            name: c.name,
            ty: f.fold_type(c.ty),
            value: c.value.map(|v| f.fold_expr(v)),
            span: c.span,
        }),
    };
    item::ImplItem {
        item: item_kind,
        defaultness: item.defaultness,
        attributes: item.attributes,
        visibility: item.visibility,
        span: item.span,
    }
}

pub fn fold_fn_sig<F: Folder + ?Sized>(f: &mut F, sig: item::FnSig) -> item::FnSig {
    item::FnSig {
        params: sig
            .params
            .into_iter()
            .map(|param| f.fold_param(param))
            .collect(),
        return_type: match sig.return_type {
            item::FnRefType::Type(ty) => item::FnRefType::Type(f.fold_type(ty)),
            item::FnRefType::Default(span) => item::FnRefType::Default(span),
        },
        is_async: sig.is_async,
        is_variadic: sig.is_variadic,
    }
}

pub fn fold_generics<F: Folder + ?Sized>(f: &mut F, generics: item::Generics) -> item::Generics {
    item::Generics {
        params: generics
            .params
            .into_iter()
            .map(|param| match param {
                item::GenericParam::Type(t) => item::GenericParam::Type(item::TypeParam {
                    name: t.name,
                    bounds: t
                        .bounds
                        .into_iter()
                        .map(|b| f.fold_trait_bound(b))
                        .collect(),
                    default: t.default.map(|ty| f.fold_type(ty)),
                    span: t.span,
                }),
                item::GenericParam::Const(c) => item::GenericParam::Const(item::ConstParam {
                    name: c.name,
                    ty: f.fold_type(c.ty),
                    default: c.default.map(|e| f.fold_expr(e)),
                    span: c.span,
                }),
            })
            .collect(),
        where_clause: generics.where_clause.map(|wc| f.fold_where_clause(wc)),
        span: generics.span,
    }
}

pub fn fold_where_clause<F: Folder + ?Sized>(
    f: &mut F,
    wc: item::WhereClause,
) -> item::WhereClause {
    fn fold_where_predicate_inner<F: Folder + ?Sized>(
        f: &mut F,
        pred: item::WherePredicate,
    ) -> item::WherePredicate {
        match pred {
            item::WherePredicate::ForAll {
                params,
                predicate,
                span,
            } => item::WherePredicate::ForAll {
                params,
                predicate: Box::new(fold_where_predicate_inner(f, *predicate)),
                span,
            },
            item::WherePredicate::TraitBound { ty, bounds } => item::WherePredicate::TraitBound {
                ty: f.fold_type(ty),
                bounds: bounds.into_iter().map(|b| f.fold_trait_bound(b)).collect(),
            },
            item::WherePredicate::TypeEq { lhs, rhs } => item::WherePredicate::TypeEq {
                lhs: f.fold_type(lhs),
                rhs: f.fold_type(rhs),
            },
        }
    }

    item::WhereClause {
        predicates: wc
            .predicates
            .into_iter()
            .map(|pred| fold_where_predicate_inner(f, pred))
            .collect(),
        span: wc.span,
    }
}

pub fn fold_trait_bound<F: Folder + ?Sized>(
    f: &mut F,
    bound: item::TraitBound,
) -> item::TraitBound {
    item::TraitBound {
        binder: bound.binder.map(|params| item::TypeBinderParams {
            params: params
                .params
                .into_iter()
                .map(|p| match p {
                    item::TypeBinderParam::Type(tp) => {
                        item::TypeBinderParam::Type(item::TypeBinderTyParam {
                            name: f.fold_ident(tp.name),
                            bounds: tp
                                .bounds
                                .into_iter()
                                .map(|b| f.fold_trait_bound(b))
                                .collect(),
                            span: tp.span,
                        })
                    }
                    item::TypeBinderParam::Const(c) => {
                        item::TypeBinderParam::Const(item::ConstBinderParam {
                            name: f.fold_ident(c.name),
                            ty: f.fold_type(c.ty),
                            span: c.span,
                        })
                    }
                })
                .collect(),
            span: params.span,
        }),
        path: f.fold_path(bound.path),
        span: bound.span,
    }
}

pub fn fold_attribute<F: Folder + ?Sized>(f: &mut F, attr: item::Attribute) -> item::Attribute {
    item::Attribute {
        is_absolute: attr.is_absolute,
        path: attr.path,
        args: match attr.args {
            item::AttributeArgs::Empty => item::AttributeArgs::Empty,
            item::AttributeArgs::Positional(exprs) => {
                item::AttributeArgs::Positional(exprs.into_iter().map(|e| f.fold_expr(e)).collect())
            }
            item::AttributeArgs::Named(args) => item::AttributeArgs::Named(
                args.into_iter()
                    .map(|arg| item::NamedArg {
                        name: arg.name,
                        value: f.fold_expr(arg.value),
                    })
                    .collect(),
            ),
        },
        span: attr.span,
    }
}

pub fn fold_path_segment<F: Folder + ?Sized>(
    f: &mut F,
    segment: expr::PathSegment,
) -> expr::PathSegment {
    expr::PathSegment {
        ident: f.fold_ident(segment.ident),
        args: segment.args.map(|args| f.fold_generic_args(args)),
    }
}

pub fn fold_ident<F: Folder + ?Sized>(f: &mut F, ident: Ident) -> Ident {
    ident
}

pub fn fold_param<F: Folder + ?Sized>(f: &mut F, param: item::Param) -> item::Param {
    item::Param {
        pattern: f.fold_pattern(param.pattern),
        ty: f.fold_type(param.ty),
        span: param.span,
    }
}

pub fn fold_field_def<F: Folder + ?Sized>(f: &mut F, field: item::FieldDef) -> item::FieldDef {
    item::FieldDef {
        attributes: field
            .attributes
            .into_iter()
            .map(|a| f.fold_attribute(a))
            .collect(),
        name: field.name,
        ty: f.fold_type(field.ty),
        visibility: field.visibility,
        span: field.span,
    }
}

pub fn fold_generic_args<F: Folder + ?Sized>(
    f: &mut F,
    args: expr::GenericArgs,
) -> expr::GenericArgs {
    match args {
        expr::GenericArgs::AngleBracketed(args) => {
            expr::GenericArgs::AngleBracketed(expr::AngleBracketedArgs {
                args: args
                    .args
                    .into_iter()
                    .map(|arg| match arg {
                        expr::AngleBracketedArg::Type(t) => {
                            expr::AngleBracketedArg::Type(f.fold_type(t))
                        }
                        expr::AngleBracketedArg::Const(e) => {
                            expr::AngleBracketedArg::Const(f.fold_expr(e))
                        }
                        expr::AngleBracketedArg::AssociatedType { name, ty } => {
                            expr::AngleBracketedArg::AssociatedType {
                                name,
                                ty: f.fold_type(ty),
                            }
                        }
                    })
                    .collect(),
                span: args.span,
            })
        }
        expr::GenericArgs::Parenthesized(args) => {
            expr::GenericArgs::Parenthesized(ParenthesizedArgs {
                ins: args.ins.into_iter().map(|t| f.fold_type(t)).collect(),
                out: args.out.map(|t| f.fold_type(t)),
                span: args.span,
            })
        }
    }
}
