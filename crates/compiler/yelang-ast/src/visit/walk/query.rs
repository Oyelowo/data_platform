use crate::{visit::walk::visitor::Visitor, *};
use std::ops::ControlFlow;

// --- Query Walkers ---

pub fn walk_query<V: Visitor>(v: &mut V, query: &Query) -> ControlFlow<()> {
    match &query.kind {
        QueryKind::Select(s) => v.visit_select_stmt(s),
        QueryKind::Create(c) => v.visit_create_stmt(c),
        QueryKind::Upsert(i) => v.visit_upsert_stmt(i),
        QueryKind::Update(u) => v.visit_update_stmt(u),
        QueryKind::Unlink(u) => v.visit_unlink_stmt(u),
        QueryKind::Link(l) => v.visit_link_stmt(l),
        QueryKind::Delete(d) => v.visit_delete_stmt(d),
    }
}

pub fn walk_select_stmt<V: Visitor>(v: &mut V, stmt: &SelectQ) -> ControlFlow<()> {
    // 1. Projection
    v.visit_expr(&stmt.projection)?;

    // 2. From
    for from in &stmt.from {
        v.visit_from_node(from)?;
    }

    // 3. Links
    for link in &stmt.links {
        v.visit_select_linkpath(link)?;
    }

    // 3b. Post-LINKS per-root tail modifiers (`for <root> { ... }`)
    for block in &stmt.post_links_for {
        v.visit_ident(&block.target)?;
        if let Some(expr) = &block.modifiers.filter {
            v.visit_expr(expr)?;
        }
        if let Some(parts) = &block.modifiers.order {
            for part in parts {
                v.visit_select_order_by_part(part)?;
            }
        }
        if let Some(range) = &block.modifiers.range {
            v.visit_select_range(range)?;
        }
    }

    // 4. Where
    if let Some(w) = &stmt.where_clause {
        v.visit_expr(w)?;
    }

    // 5. Group By
    if let Some(g) = &stmt.group_by {
        for k in &g.keys {
            v.visit_expr(&k.expr)?;
        }
    }

    // 6. Order By
    if let Some(o) = &stmt.order_by {
        for p in o {
            v.visit_select_order_by_part(p)?;
        }
    }

    // 7. Range
    if let Some(r) = &stmt.range {
        v.visit_select_range(r)?;
    }

    ControlFlow::Continue(())
}

pub fn walk_create_stmt<V: Visitor>(v: &mut V, stmt: &CreateQ) -> ControlFlow<()> {
    v.visit_ident(&stmt.var)?;
    v.visit_ident(&stmt.binding)?;
    v.visit_type(&stmt.table)?;

    match &stmt.data {
        CreationData::Object(obj) => v.visit_object(obj)?,
        CreationData::Array(arr) => v.visit_array(arr)?,
    }

    for link in &stmt.links {
        v.visit_create_path(link)?;
    }

    if let Some(ret) = &stmt.return_ {
        v.visit_expr(ret)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_upsert_stmt<V: Visitor>(v: &mut V, stmt: &UpsertQ) -> ControlFlow<()> {
    v.visit_ident(&stmt.var)?;
    v.visit_ident(&stmt.binding)?;
    v.visit_type(&stmt.table)?;

    match &stmt.data {
        CreationData::Object(obj) => v.visit_object(obj)?,
        CreationData::Array(arr) => v.visit_array(arr)?,
    }

    if let Some(conflict) = &stmt.on_conflict {
        for field in &conflict.fields {
            v.visit_ident(field)?;
        }
    }

    for link in &stmt.links {
        v.visit_create_path(link)?;
    }

    if let Some(ret) = &stmt.return_ {
        v.visit_expr(ret)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_update_stmt<V: Visitor>(v: &mut V, stmt: &UpdateQ) -> ControlFlow<()> {
    v.visit_ident(&stmt.var)?;
    v.visit_ident(&stmt.binding)?;
    v.visit_type(&stmt.table)?;

    match &stmt.mutation {
        UpdateMutation::Merge(obj) => v.visit_object(obj)?,
        UpdateMutation::Set(setters) => {
            for setter in setters {
                v.visit_expr(&setter.path)?;
                v.visit_expr(&setter.value)?;
            }
        }
    }

    for link in &stmt.links {
        v.visit_create_path(link)?;
    }

    if let Some(cond) = &stmt.condition {
        v.visit_expr(cond)?;
    }

    if let Some(ret) = &stmt.return_ {
        v.visit_expr(ret)?;
    }

    ControlFlow::Continue(())
}

pub fn walk_unlink_stmt<V: Visitor>(v: &mut V, stmt: &UnlinkQ) -> ControlFlow<()> {
    for path in &stmt.paths {
        v.visit_select_linkpath(path)?;
    }
    if let Some(ret) = &stmt.return_ {
        v.visit_expr(ret)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_link_stmt<V: Visitor>(v: &mut V, stmt: &LinkQ) -> ControlFlow<()> {
    for path in &stmt.paths {
        v.visit_create_path(path)?;
    }
    if let Some(ret) = &stmt.return_ {
        v.visit_expr(ret)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_create_path<V: Visitor>(v: &mut V, path: &CreatePath) -> ControlFlow<()> {
    for segment in &path.segments {
        walk_create_path_segment(v, segment)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_create_path_segment<V: Visitor>(
    v: &mut V,
    segment: &CreatePathSegment,
) -> ControlFlow<()> {
    match segment {
        CreatePathSegment::Node(node) => v.visit_select_node(node),
        CreatePathSegment::Edge(edge) => v.visit_create_edge(edge),
    }
}

pub fn walk_create_edge<V: Visitor>(v: &mut V, edge: &CreateEdge) -> ControlFlow<()> {
    v.visit_ident(&edge.var)?;
    v.visit_ident(&edge.binding)?;
    v.visit_type(&edge.table)?;
    v.visit_object(&edge.data)?;
    ControlFlow::Continue(())
}

pub fn walk_delete_stmt<V: Visitor>(v: &mut V, stmt: &DeleteQ) -> ControlFlow<()> {
    v.visit_ident(&stmt.var)?;
    v.visit_ident(&stmt.binding)?;
    v.visit_type(&stmt.table)?;

    if let Some(cond) = &stmt.condition {
        v.visit_expr(cond)?;
    }
    if let Some(ret) = &stmt.return_ {
        v.visit_expr(ret)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_modifiers<V: Visitor>(v: &mut V, mods: &Modifiers) -> ControlFlow<()> {
    if let Some(f) = &mods.filter {
        v.visit_expr(f)?;
    }
    if let Some(o) = &mods.order {
        for p in o {
            v.visit_expr(&p.field)?;
        }
    }
    if let Some(r) = &mods.range {
        if let Some(start) = &r.start {
            v.visit_expr(start)?;
        }
        if let Some(end) = &r.end {
            v.visit_expr(end)?;
        }
    }
    ControlFlow::Continue(())
}

pub fn walk_link_path<V: Visitor>(v: &mut V, path: &LinkPath) -> ControlFlow<()> {
    // `LinkPath` is the select-style link path; reuse the canonical walker.
    walk_select_linkpath(v, path)
}

pub fn walk_hop_range<V: Visitor>(v: &mut V, range: &query::HopRange) -> ControlFlow<()> {
    if let Some(start) = &range.start {
        v.visit_expr(start)?;
    }
    if let Some(end) = &range.end {
        v.visit_expr(end)?;
    }
    ControlFlow::Continue(())
}

// --- Select Component Walkers ---

pub fn walk_from_node<V: Visitor>(v: &mut V, node: &query::FromNode) -> ControlFlow<()> {
    if let Some(var) = &node.var {
        v.visit_ident(var)?;
    }
    if let Some(bind) = &node.bind {
        v.visit_ident(bind)?;
    }
    if let Some(ty) = &node.ty {
        v.visit_type(ty)?;
    }
    v.visit_select_modifiers(&node.modifiers)
}

pub fn walk_select_node<V: Visitor>(v: &mut V, node: &query::Node) -> ControlFlow<()> {
    if let Some(var) = &node.var {
        v.visit_ident(var)?;
    }
    if let Some(bind) = &node.bind {
        v.visit_ident(bind)?;
    }
    if let Some(ty) = &node.ty {
        v.visit_type(ty)?;
    }
    v.visit_select_modifiers(&node.modifiers)
}

pub fn walk_select_linkpath<V: Visitor>(v: &mut V, path: &query::LinkPath) -> ControlFlow<()> {
    v.visit_select_node(&path.start)?;
    for segment in &path.segments {
        v.visit_select_linksegment(segment)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_select_linksegment<V: Visitor>(
    v: &mut V,
    segment: &query::LinkSegment,
) -> ControlFlow<()> {
    v.visit_select_edge(&segment.edge)?;
    v.visit_select_node(&segment.target)
}

pub fn walk_select_edge<V: Visitor>(v: &mut V, edge: &query::Edge) -> ControlFlow<()> {
    if let Some(var) = &edge.var {
        v.visit_ident(var)?;
    }
    if let Some(bind) = &edge.bind {
        v.visit_ident(bind)?;
    }
    if let Some(ty) = &edge.ty {
        v.visit_type(ty)?;
    }
    if let Some(h) = &edge.hops {
        v.visit_hop_range(h)?;
    }
    v.visit_select_modifiers(&edge.modifiers)
}

pub fn walk_select_order_by_part<V: Visitor>(
    v: &mut V,
    part: &query::OrderByPart,
) -> ControlFlow<()> {
    v.visit_expr(&part.field)
}

pub fn walk_select_modifiers<V: Visitor>(v: &mut V, mods: &query::Modifiers) -> ControlFlow<()> {
    if let Some(f) = &mods.filter {
        v.visit_expr(f)?;
    }
    if let Some(o) = &mods.order {
        for p in o {
            v.visit_expr(&p.field)?;
        }
    }
    if let Some(r) = &mods.range {
        if let Some(start) = &r.start {
            v.visit_expr(start)?;
        }
        if let Some(end) = &r.end {
            v.visit_expr(end)?;
        }
    }
    ControlFlow::Continue(())
}
