use crate::{
    common::{self, *},
    expr::{self, *},
    item::{self, *},
    pattern::{self, *},
    ptr::{self, *},
    query::{self, Range as QueryRange, *},
    stmt::{self, *},
    tokenizer::{self, *},
    types::{self, *},
    visit::fold::folder::Folder,
};

use crate::query::{CreatePathSegment, CreationData, UpdateMutation};

pub fn fold_query<F: Folder + ?Sized>(f: &mut F, query: Query) -> Query {
    let kind = match query.kind {
        QueryKind::Select(s) => QueryKind::Select(Box::new(f.fold_select_stmt(*s))),
        QueryKind::Create(c) => QueryKind::Create(f.fold_create_stmt(c)),
        QueryKind::Upsert(i) => QueryKind::Upsert(f.fold_upsert_stmt(i)),
        QueryKind::Update(u) => QueryKind::Update(f.fold_update_stmt(u)),
        QueryKind::Unlink(u) => QueryKind::Unlink(f.fold_unlink_stmt(u)),
        QueryKind::Link(l) => QueryKind::Link(f.fold_link_stmt(l)),
        QueryKind::Delete(d) => QueryKind::Delete(f.fold_delete_stmt(d)),
    };

    Query {
        kind,
        span: query.span,
    }
}

pub fn fold_select_stmt<F: Folder + ?Sized>(f: &mut F, stmt: SelectQ) -> SelectQ {
    SelectQ {
        projection: f.fold_expr(stmt.projection),
        from: stmt.from.into_iter().map(|n| f.fold_from_node(n)).collect(),
        links_match_kind: stmt.links_match_kind,
        links: stmt
            .links
            .into_iter()
            .map(|l| f.fold_select_linkpath(l))
            .collect(),
        post_links_for: stmt
            .post_links_for
            .into_iter()
            .map(|b| ForRootModifiers {
                target: f.fold_ident(b.target),
                modifiers: fold_modifiers(f, b.modifiers),
            })
            .collect(),
        where_clause: stmt.where_clause.map(|w| f.fold_expr(w)),
        group_by: stmt.group_by.map(|g| crate::query::GroupByClause {
            keys: g
                .keys
                .into_iter()
                .map(|k| crate::query::GroupByKey {
                    name: k.name,
                    expr: f.fold_expr(k.expr),
                })
                .collect(),
            into: g.into,
        }),
        order_by: stmt.order_by.map(|o| {
            o.into_iter()
                .map(|p| f.fold_select_order_by_part(p))
                .collect()
        }),
        range: stmt.range.map(|r| f.fold_select_range(r)),
    }
}

pub fn fold_create_stmt<F: Folder + ?Sized>(f: &mut F, stmt: CreateQ) -> CreateQ {
    use crate::expr::ArrayKind;
    CreateQ {
        var: stmt.var,
        binding: stmt.binding,
        table: f.fold_type(stmt.table),
        data: match stmt.data {
            CreationData::Object(obj) => CreationData::Object(Object {
                fields: obj
                    .fields
                    .into_iter()
                    .map(|field| ObjectField {
                        key: field.key,
                        val: f.fold_expr(field.val),
                    })
                    .collect(),
                span: obj.span,
            }),
            CreationData::Array(arr) => {
                let kind = match arr.kind {
                    ArrayKind::List(elements) => {
                        ArrayKind::List(elements.into_iter().map(|e| f.fold_expr(e)).collect())
                    }
                    ArrayKind::Repeat { value, count } => ArrayKind::Repeat {
                        value: Box::new(f.fold_expr(*value)),
                        count: Box::new(f.fold_expr(*count)),
                    },
                };
                CreationData::Array(Array { kind })
            }
        },
        links: stmt
            .links
            .into_iter()
            .map(|p| fold_create_path(f, p))
            .collect(),
        return_: stmt.return_.map(|e| f.fold_expr(e)),
        span: stmt.span,
    }
}

pub fn fold_upsert_stmt<F: Folder + ?Sized>(f: &mut F, stmt: UpsertQ) -> UpsertQ {
    use crate::expr::ArrayKind;
    UpsertQ {
        var: stmt.var,
        binding: stmt.binding,
        table: f.fold_type(stmt.table),
        data: match stmt.data {
            CreationData::Object(obj) => CreationData::Object(Object {
                fields: obj
                    .fields
                    .into_iter()
                    .map(|field| ObjectField {
                        key: field.key,
                        val: f.fold_expr(field.val),
                    })
                    .collect(),
                span: obj.span,
            }),
            CreationData::Array(arr) => {
                let kind = match arr.kind {
                    ArrayKind::List(elements) => {
                        ArrayKind::List(elements.into_iter().map(|e| f.fold_expr(e)).collect())
                    }
                    ArrayKind::Repeat { value, count } => ArrayKind::Repeat {
                        value: Box::new(f.fold_expr(*value)),
                        count: Box::new(f.fold_expr(*count)),
                    },
                };
                CreationData::Array(Array { kind })
            }
        },
        on_conflict: stmt.on_conflict.map(|conflict| ConflictClause {
            fields: conflict
                .fields
                .into_iter()
                .map(|field| f.fold_ident(field))
                .collect(),
            action: conflict.action,
        }),
        links: stmt
            .links
            .into_iter()
            .map(|p| fold_create_path(f, p))
            .collect(),
        return_: stmt.return_.map(|e| f.fold_expr(e)),
        span: stmt.span,
    }
}

pub fn fold_update_stmt<F: Folder + ?Sized>(f: &mut F, stmt: UpdateQ) -> UpdateQ {
    UpdateQ {
        var: f.fold_ident(stmt.var),
        binding: f.fold_ident(stmt.binding),
        table: f.fold_type(stmt.table),
        mutation: match stmt.mutation {
            UpdateMutation::Merge(obj) => UpdateMutation::Merge(Object {
                fields: obj
                    .fields
                    .into_iter()
                    .map(|field| ObjectField {
                        key: field.key,
                        val: f.fold_expr(field.val),
                    })
                    .collect(),
                span: obj.span,
            }),
            UpdateMutation::Set(setters) => UpdateMutation::Set(
                setters
                    .into_iter()
                    .map(|s| Setter {
                        path: f.fold_expr(s.path),
                        op: s.op,
                        value: f.fold_expr(s.value),
                    })
                    .collect(),
            ),
        },
        links: stmt
            .links
            .into_iter()
            .map(|p| fold_create_path(f, p))
            .collect(),
        condition: stmt.condition.map(|e| f.fold_expr(e)),
        return_: stmt.return_.map(|e| f.fold_expr(e)),
        span: stmt.span,
    }
}

pub fn fold_unlink_stmt<F: Folder + ?Sized>(f: &mut F, stmt: UnlinkQ) -> UnlinkQ {
    UnlinkQ {
        paths: stmt
            .paths
            .into_iter()
            .map(|p| f.fold_select_linkpath(p))
            .collect(),
        return_: stmt.return_.map(|e| f.fold_expr(e)),
    }
}

pub fn fold_link_stmt<F: Folder + ?Sized>(f: &mut F, stmt: LinkQ) -> LinkQ {
    LinkQ {
        paths: stmt
            .paths
            .into_iter()
            .map(|p| fold_create_path(f, p))
            .collect(),
        return_: stmt.return_.map(|e| f.fold_expr(e)),
        span: stmt.span,
    }
}

pub fn fold_create_path<F: Folder + ?Sized>(f: &mut F, path: CreatePath) -> CreatePath {
    CreatePath {
        segments: path
            .segments
            .into_iter()
            .map(|s| fold_create_path_segment(f, s))
            .collect(),
    }
}

pub fn fold_create_path_segment<F: Folder + ?Sized>(
    f: &mut F,
    segment: CreatePathSegment,
) -> CreatePathSegment {
    match segment {
        CreatePathSegment::Node(node) => CreatePathSegment::Node(f.fold_select_node(node)),
        CreatePathSegment::Edge(edge) => CreatePathSegment::Edge(fold_create_edge(f, edge)),
    }
}

pub fn fold_create_edge<F: Folder + ?Sized>(f: &mut F, edge: CreateEdge) -> CreateEdge {
    CreateEdge {
        var: f.fold_ident(edge.var),
        binding: f.fold_ident(edge.binding),
        table: f.fold_type(edge.table),
        data: f.fold_object(edge.data),
        direction: edge.direction,
    }
}

pub fn fold_delete_stmt<F: Folder + ?Sized>(f: &mut F, stmt: DeleteQ) -> DeleteQ {
    DeleteQ {
        var: f.fold_ident(stmt.var),
        binding: f.fold_ident(stmt.binding),
        table: f.fold_type(stmt.table),
        condition: stmt.condition.map(|e| f.fold_expr(e)),
        return_: stmt.return_.map(|e| f.fold_expr(e)),
        span: stmt.span,
    }
}

// ========================================================================
// SELECT COMPONENT FOLDERS
// ========================================================================

pub fn fold_modifiers<F: Folder + ?Sized>(f: &mut F, mods: Modifiers) -> Modifiers {
    Modifiers {
        filter: mods.filter.map(|e| f.fold_expr(e)),
        order: mods.order.map(|o| {
            o.into_iter()
                .map(|p| f.fold_select_order_by_part(p))
                .collect()
        }),
        range: mods.range.map(|r| QueryRange {
            start: r.start.map(|e| f.fold_expr(e)),
            end: r.end.map(|e| f.fold_expr(e)),
            inclusive: r.inclusive,
        }),
    }
}
