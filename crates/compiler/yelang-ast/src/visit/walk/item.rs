/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2025 Oyelowo Oyedayo
 */

use crate::{expr, item, visit::walk::visitor::Visitor, *};
use std::ops::ControlFlow;

// --- Item Walkers ---

pub fn walk_item<V: Visitor>(v: &mut V, item: &Item) -> ControlFlow<()> {
    // Visit attributes
    for attr in &item.attributes {
        v.visit_attribute(attr)?;
    }

    match &item.kind {
        ItemKind::Fn(f) => v.visit_fn(f),
        ItemKind::Struct(s) => v.visit_struct(s),
        ItemKind::Enum(e) => v.visit_enum(e),
        ItemKind::Trait(t) => v.visit_trait(t),
        ItemKind::Impl(i) => v.visit_impl(i),
        ItemKind::Module(m) => v.visit_module(m),
        ItemKind::TypeAlias(ta) => v.visit_type_alias(ta),
        ItemKind::Const(c) => v.visit_const(c),
        ItemKind::Static(s) => v.visit_static(s),
        ItemKind::Use(u) => v.visit_use(u),
        ItemKind::MacroDef(_) => ControlFlow::Continue(()),
    }
}

pub fn walk_fn<V: Visitor>(v: &mut V, func: &item::FnDef) -> ControlFlow<()> {
    v.visit_ident(&func.name)?;
    v.visit_generics(&func.generics)?;
    if let Some(where_clause) = &func.generics.where_clause {
        v.visit_where_clause(where_clause)?;
    }

    // Visit params
    for param in &func.sig.params {
        v.visit_param(param)?;
    }

    // Visit return type
    if let item::FnRefType::Type(ty) = &func.sig.return_type {
        v.visit_type(ty)?;
    }

    v.visit_block_expr(&func.body)
}

pub fn walk_struct<V: Visitor>(v: &mut V, s: &item::Struct) -> ControlFlow<()> {
    v.visit_ident(&s.name)?;
    v.visit_generics(&s.generics)?;
    if let Some(where_clause) = &s.generics.where_clause {
        v.visit_where_clause(where_clause)?;
    }

    match &s.fields {
        StructFields::Named(fields) => {
            for field in fields {
                v.visit_field_def(field)?;
            }
        }
        StructFields::Tuple(tys) => {
            for ty in tys {
                v.visit_type(ty)?;
            }
        }
        StructFields::Unit => {}
    }
    ControlFlow::Continue(())
}

pub fn walk_enum<V: Visitor>(v: &mut V, e: &item::Enum) -> ControlFlow<()> {
    v.visit_ident(&e.name)?;
    v.visit_generics(&e.generics)?;
    if let Some(where_clause) = &e.generics.where_clause {
        v.visit_where_clause(where_clause)?;
    }

    for variant in &e.variants {
        // Visit variant attributes (decorators)
        for attr in &variant.attributes {
            v.visit_attribute(attr)?;
        }
        v.visit_ident(&variant.name)?;
        match &variant.kind {
            item::VariantKind::Struct(fields) => {
                for field in fields {
                    v.visit_field_def(field)?;
                }
            }
            item::VariantKind::Tuple(tys) => {
                for ty in tys {
                    v.visit_type(ty)?;
                }
            }
            item::VariantKind::Unit => {}
        }
        if let Some(disc) = &variant.discriminant {
            v.visit_expr(disc)?;
        }
    }
    ControlFlow::Continue(())
}

pub fn walk_trait<V: Visitor>(v: &mut V, t: &item::Trait) -> ControlFlow<()> {
    v.visit_ident(&t.name)?;
    v.visit_generics(&t.generics)?;
    if let Some(where_clause) = &t.generics.where_clause {
        v.visit_where_clause(where_clause)?;
    }

    for bound in &t.super_traits {
        v.visit_path(&bound.path)?;
    }

    for item in &t.items {
        v.visit_trait_item(item)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_impl<V: Visitor>(v: &mut V, i: &item::Impl) -> ControlFlow<()> {
    // Visit impl attributes (decorators)
    for attr in &i.attributes {
        v.visit_attribute(attr)?;
    }
    v.visit_generics(&i.generics)?;
    if let Some(where_clause) = &i.generics.where_clause {
        v.visit_where_clause(where_clause)?;
    }
    if let Some(trait_path) = &i.trait_impl {
        v.visit_path(trait_path)?;
    }

    v.visit_type(&i.self_ty)?;

    for item in &i.items {
        v.visit_impl_item(item)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_module<V: Visitor>(v: &mut V, m: &item::ModDef) -> ControlFlow<()> {
    v.visit_ident(&m.name)?;
    if let item::ModKind::Inline { items } = &m.kind {
        for item in items {
            v.visit_item(item)?;
        }
    }
    ControlFlow::Continue(())
}

pub fn walk_type_alias<V: Visitor>(v: &mut V, ta: &item::TypeAlias) -> ControlFlow<()> {
    v.visit_ident(&ta.name)?;
    v.visit_generics(&ta.generics)?;
    if let Some(where_clause) = &ta.generics.where_clause {
        v.visit_where_clause(where_clause)?;
    }
    v.visit_type(&ta.target)
}

pub fn walk_const<V: Visitor>(v: &mut V, c: &item::Const) -> ControlFlow<()> {
    v.visit_ident(&c.name)?;
    v.visit_type(&c.ty)?;
    v.visit_expr(&c.value)
}

pub fn walk_static<V: Visitor>(v: &mut V, s: &item::Static) -> ControlFlow<()> {
    v.visit_ident(&s.name)?;
    v.visit_type(&s.ty)?;
    v.visit_expr(&s.value)
}

// --- Type & Pattern Walkers ---

pub fn walk_type<V: Visitor>(v: &mut V, ty: &Type) -> ControlFlow<()> {
    match &ty.kind {
        TypeKind::Array(t, len) => {
            v.visit_type(t)?;
            v.visit_expr(len)
        }
        TypeKind::Tuple(tys) => {
            for t in tys {
                v.visit_type(t)?;
            }
            ControlFlow::Continue(())
        }
        TypeKind::Slice(t) => v.visit_type(t),
        TypeKind::Ref { ty, .. } => v.visit_type(ty),
        TypeKind::Named(path) => {
            // Generics are in path.segments[].args and will be visited via visit_path
            v.visit_path(path)
        }
        TypeKind::Function(func_ty) => {
            for param in &func_ty.params {
                v.visit_type(param)?;
            }
            v.visit_type(&func_ty.return_type)
        }
        TypeKind::ForAll { params, ty } => {
            for p in &params.params {
                match p {
                    crate::item::TypeBinderParam::Type(tp) => {
                        v.visit_ident(&tp.name)?;
                        for b in &tp.bounds {
                            v.visit_trait_bound(b)?;
                        }
                    }
                    crate::item::TypeBinderParam::Const(c) => {
                        v.visit_ident(&c.name)?;
                        v.visit_type(&c.ty)?;
                    }
                }
            }
            v.visit_type(ty)
        }
        TypeKind::Structural(fields) => {
            for field in fields {
                v.visit_ident(&field.name)?;
                v.visit_type(&field.ty)?;
            }
            ControlFlow::Continue(())
        }
        TypeKind::Never => ControlFlow::Continue(()),
        TypeKind::Infer => ControlFlow::Continue(()),
        TypeKind::Union(types) => {
            for t in types {
                v.visit_type(t)?;
            }
            ControlFlow::Continue(())
        }
        TypeKind::Literal(_) => ControlFlow::Continue(()),
        TypeKind::Operator(op) => match op {
            TypeOperator::TypeOf(expr) => v.visit_expr(expr),
            TypeOperator::ReturnType(inner_ty) | TypeOperator::Parameters(inner_ty) => {
                v.visit_type(inner_ty)
            }
            TypeOperator::Pick(base, keys) | TypeOperator::Omit(base, keys) => {
                v.visit_type(base)?;
                v.visit_type(keys)
            }
        },
        TypeKind::ImplTrait(path) | TypeKind::DynTrait(path) => v.visit_path(path),
        TypeKind::Error => ControlFlow::Continue(()),
    }
}

pub fn walk_pattern<V: Visitor>(v: &mut V, pat: &Pattern) -> ControlFlow<()> {
    match &pat.pattern {
        PatternKind::Binding {
            name, subpattern, ..
        } => {
            v.visit_ident(name)?;
            if let Some(pat) = subpattern.as_deref() {
                v.visit_pattern(pat)?;
            }
            ControlFlow::Continue(())
        }
        PatternKind::Tuple { patterns }
        | PatternKind::Slice { patterns }
        | PatternKind::Or(patterns) => {
            for p in patterns {
                v.visit_pattern(p)?;
            }
            ControlFlow::Continue(())
        }
        PatternKind::Ref { pattern, .. } => v.visit_pattern(pattern),
        PatternKind::Struct { path, fields, rest } => {
            v.visit_path(path)?;
            for f in fields {
                v.visit_field_pattern(f)?;
            }
            ControlFlow::Continue(())
        }
        PatternKind::Record { fields, .. } => {
            for f in fields {
                v.visit_field_pattern(f)?;
            }
            ControlFlow::Continue(())
        }
        PatternKind::Absent => ControlFlow::Continue(()),
        PatternKind::Wildcard => ControlFlow::Continue(()),
        PatternKind::Path(path) => v.visit_path(path),
        PatternKind::Literal(_) => ControlFlow::Continue(()),
        PatternKind::TupleStruct { path, patterns } => {
            v.visit_path(path)?;
            for p in patterns {
                v.visit_pattern(p)?;
            }
            ControlFlow::Continue(())
        }
        PatternKind::Rest { name: _ } => ControlFlow::Continue(()),
        PatternKind::Range(range) => {
            if let Some(start) = &range.start {
                v.visit_expr(start)?;
            }
            if let Some(end) = &range.end {
                v.visit_expr(end)?;
            }
            ControlFlow::Continue(())
        }
        PatternKind::Grouped(pat) => v.visit_pattern(pat),
    }
}

pub fn walk_field_pattern<V: Visitor>(v: &mut V, field: &FieldPattern) -> ControlFlow<()> {
    v.visit_ident(&field.name)?;
    v.visit_pattern(&field.pattern)
}

pub fn walk_trait_item<V: Visitor>(v: &mut V, item: &item::TraitItem) -> ControlFlow<()> {
    // Visit trait item attributes (decorators)
    for attr in &item.attributes {
        v.visit_attribute(attr)?;
    }
    match &item.item {
        item::TraitItemKind::Method(m) => {
            v.visit_ident(&m.segment)?;
            // Visit parameters
            for param in &m.sig.params {
                v.visit_param(param)?;
            }
            // Visit return type
            match &m.sig.return_type {
                item::FnRefType::Type(ty) => v.visit_type(&ty)?,
                item::FnRefType::Default(_) => {}
            }
            // Visit body if present
            if let Some(body) = &m.body {
                v.visit_block_expr(body)?;
            }
            ControlFlow::Continue(())
        }
        item::TraitItemKind::AssociatedType(t) => {
            v.visit_ident(&t.name)?;
            v.visit_generics(&t.generics)?;
            if let Some(ty) = &t.default {
                v.visit_type(ty)?;
            }
            ControlFlow::Continue(())
        }
        item::TraitItemKind::Constant(c) => {
            v.visit_ident(&c.name)?;
            v.visit_type(&c.ty)?;
            if let Some(val) = &c.value {
                v.visit_expr(val)?;
            }
            ControlFlow::Continue(())
        }
    }
}

pub fn walk_impl_item<V: Visitor>(v: &mut V, item: &item::ImplItem) -> ControlFlow<()> {
    // Visit impl item attributes (decorators)
    for attr in &item.attributes {
        v.visit_attribute(attr)?;
    }
    match &item.item {
        item::ImplItemKind::Method(m) => {
            v.visit_ident(&m.name)?;
            // Visit parameters
            for param in &m.sig.params {
                v.visit_param(param)?;
            }
            // Visit return type
            match &m.sig.return_type {
                item::FnRefType::Type(ty) => v.visit_type(&ty)?,
                item::FnRefType::Default(_) => {}
            }
            v.visit_block_expr(&m.body)?;
            ControlFlow::Continue(())
        }
        item::ImplItemKind::AssociatedType(t) => {
            v.visit_ident(&t.name)?;
            v.visit_generics(&t.generics)?;
            v.visit_type(&t.ty)?;
            ControlFlow::Continue(())
        }
        item::ImplItemKind::Constant(c) => {
            v.visit_ident(&c.name)?;
            v.visit_type(&c.ty)?;
            if let Some(val) = &c.value {
                v.visit_expr(val)?;
            }
            ControlFlow::Continue(())
        }
    }
}

pub fn walk_generics<V: Visitor>(v: &mut V, generics: &item::Generics) -> ControlFlow<()> {
    for param in &generics.params {
        match param {
            item::GenericParam::Type(t) => {
                v.visit_ident(&t.name)?;
                for bound in &t.bounds {
                    v.visit_trait_bound(bound)?;
                }
                if let Some(default) = &t.default {
                    v.visit_type(default)?;
                }
            }
            item::GenericParam::Const(c) => {
                v.visit_ident(&c.name)?;
                v.visit_type(&c.ty)?;
                if let Some(default) = &c.default {
                    v.visit_expr(default)?;
                }
            }
        }
    }
    ControlFlow::Continue(())
}

pub fn walk_where_clause<V: Visitor>(v: &mut V, wc: &item::WhereClause) -> ControlFlow<()> {
    for pred in &wc.predicates {
        walk_where_predicate(v, pred)?;
    }
    ControlFlow::Continue(())
}

fn visit_type_binder_params<V: Visitor>(
    v: &mut V,
    params: &[item::TypeBinderParam],
) -> ControlFlow<()> {
    for p in params {
        match p {
            item::TypeBinderParam::Type(tp) => {
                v.visit_ident(&tp.name)?;
                for b in &tp.bounds {
                    v.visit_trait_bound(b)?;
                }
            }
            item::TypeBinderParam::Const(c) => {
                v.visit_ident(&c.name)?;
                v.visit_type(&c.ty)?;
            }
        }
    }
    ControlFlow::Continue(())
}

fn walk_where_predicate<V: Visitor>(v: &mut V, pred: &item::WherePredicate) -> ControlFlow<()> {
    // A where-predicate may have nested `forall` binders.
    // Walk binders iteratively to keep control-flow shallow.
    let mut cur = pred;
    loop {
        match cur {
            item::WherePredicate::ForAll {
                params, predicate, ..
            } => {
                visit_type_binder_params(v, &params.params)?;
                cur = predicate.as_ref();
            }
            item::WherePredicate::TraitBound { ty, bounds } => {
                v.visit_type(ty)?;
                for bound in bounds {
                    if let Some(binder) = &bound.binder {
                        visit_type_binder_params(v, &binder.params)?;
                    }
                    v.visit_path(&bound.path)?;
                }
                break;
            }
            item::WherePredicate::TypeEq { lhs, rhs } => {
                v.visit_type(lhs)?;
                v.visit_type(rhs)?;
                break;
            }
        }
    }

    ControlFlow::Continue(())
}

pub fn walk_trait_bound<V: Visitor>(v: &mut V, bound: &item::TraitBound) -> ControlFlow<()> {
    if let Some(binder) = &bound.binder {
        visit_type_binder_params(v, &binder.params)?;
    }

    v.visit_path(&bound.path)
}

pub fn walk_attribute<V: Visitor>(v: &mut V, attr: &item::Attribute) -> ControlFlow<()> {
    for segment in &attr.path {
        v.visit_ident(segment)?;
    }
    match &attr.args {
        item::AttributeArgs::Empty => {}
        item::AttributeArgs::Positional(exprs) => {
            for e in exprs {
                v.visit_expr(e)?;
            }
        }
        item::AttributeArgs::Named(args) => {
            for arg in args {
                v.visit_ident(&arg.name)?;
                v.visit_expr(&arg.value)?;
            }
        }
    }
    ControlFlow::Continue(())
}

pub fn walk_path_segment<V: Visitor>(v: &mut V, segment: &expr::PathSegment) -> ControlFlow<()> {
    v.visit_ident(&segment.ident)?;
    if let Some(args) = &segment.args {
        v.visit_generic_args(args)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_param<V: Visitor>(v: &mut V, param: &item::Param) -> ControlFlow<()> {
    v.visit_pattern(&param.pattern)?;
    v.visit_type(&param.ty)
}

pub fn walk_field_def<V: Visitor>(v: &mut V, field: &item::FieldDef) -> ControlFlow<()> {
    // Visit field attributes (decorators)
    for attr in &field.attributes {
        v.visit_attribute(attr)?;
    }
    v.visit_ident(&field.name)?;
    v.visit_type(&field.ty)
}

pub fn walk_generic_args<V: Visitor>(v: &mut V, args: &expr::GenericArgs) -> ControlFlow<()> {
    match args {
        expr::GenericArgs::AngleBracketed(args) => {
            for arg in &args.args {
                match arg {
                    expr::AngleBracketedArg::Type(t) => v.visit_type(t)?,
                    expr::AngleBracketedArg::Const(e) => v.visit_expr(e)?,
                    expr::AngleBracketedArg::AssociatedType { name, ty } => {
                        v.visit_ident(name)?;
                        v.visit_type(ty)?;
                    }
                }
            }
        }
        expr::GenericArgs::Parenthesized(args) => {
            for input in &args.ins {
                v.visit_type(input)?;
            }
            if let Some(out) = &args.out {
                v.visit_type(out)?;
            }
        }
    }
    ControlFlow::Continue(())
}

pub fn walk_use_tree<V: Visitor>(v: &mut V, tree: &item::UseTree) -> ControlFlow<()> {
    match tree {
        item::UseTree::Simple { path, .. } => v.visit_path(path),
        item::UseTree::Rename { path, alias, .. } => {
            v.visit_path(path)?;
            v.visit_ident(alias)
        }
        item::UseTree::Glob { path, .. } => v.visit_path(path),
        item::UseTree::Nested { prefix, items, .. } => {
            v.visit_path(prefix)?;
            for item in items {
                v.visit_use_tree(item)?;
            }
            ControlFlow::Continue(())
        }
    }
}
