use crate::QueryKind;
use crate::query::{
    CreateEdge, CreatePath, CreatePathSegment, CreateQ, CreationData, DeleteQ, LinkPath, LinkQ,
    Modifiers, Node, OrderByPart, Query, Range, Setter, SortDirection, UnlinkQ, UpdateMutation,
    UpdateQ, UpsertQ,
};
use crate::{Codegen, Interner};
use std::fmt::{self, Write};

// --- Query Expressions ---

impl Codegen for Query {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match &self.kind {
            QueryKind::Select(q) => q.codegen(f, interner),
            QueryKind::Create(q) => q.codegen(f, interner),
            QueryKind::Upsert(q) => q.codegen(f, interner),
            QueryKind::Update(q) => q.codegen(f, interner),
            QueryKind::Link(q) => q.codegen(f, interner),
            QueryKind::Unlink(q) => q.codegen(f, interner),
            QueryKind::Delete(q) => q.codegen(f, interner),
        }
    }
}

// --- Query Types ---

impl Codegen for LinkQ {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "link ")?;
        for (i, path) in self.paths.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            path.codegen(f, interner)?;
        }
        if let Some(return_) = &self.return_ {
            write!(f, " return ")?;
            return_.codegen(f, interner)?;
        }
        Ok(())
    }
}

impl Codegen for CreatePath {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        for segment in &self.segments {
            segment.codegen(f, interner)?;
        }
        Ok(())
    }
}

impl Codegen for CreatePathSegment {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match self {
            CreatePathSegment::Node(node) => node.codegen(f, interner),
            CreatePathSegment::Edge(edge) => {
                write!(f, " ")?;
                edge.codegen(f, interner)?;
                Ok(())
            }
        }
    }
}

impl Codegen for CreateEdge {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        // Output: DIRECTION [edge] DIRECTION
        write!(f, "{}", self.direction)?;
        write!(f, "[")?;
        self.var.codegen(f, interner)?;
        write!(f, "@")?;
        self.binding.codegen(f, interner)?;
        write!(f, ":")?;
        self.table.codegen(f, interner)?;
        write!(f, " ")?;
        self.data.codegen(f, interner)?;
        write!(f, "]")?;
        write!(f, "{}", self.direction)?;
        write!(f, " ")?;
        Ok(())
    }
}

impl Codegen for CreateQ {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        let is_block = self.return_.is_some();
        if is_block {
            write!(f, "create {{ ")?;
        } else {
            write!(f, "create ")?;
        }

        write!(
            f,
            "{}@{}:",
            interner.resolve(&self.var.symbol),
            interner.resolve(&self.binding.symbol)
        )?;
        self.table.codegen(f, interner)?;
        write!(f, " ")?;
        match &self.data {
            CreationData::Object(obj) => obj.codegen(f, interner)?,
            CreationData::Array(arr) => arr.codegen(f, interner)?,
        }
        if !self.links.is_empty() {
            write!(f, " link ")?;
            for (i, path) in self.links.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                path.codegen(f, interner)?;
            }
        }
        if let Some(tail) = &self.return_ {
            write!(f, "; ")?;
            tail.codegen(f, interner)?;
            write!(f, " }}")?;
        }
        Ok(())
    }
}

impl Codegen for UpsertQ {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        let is_block = self.return_.is_some();
        if is_block {
            write!(f, "upsert {{ ")?;
        } else {
            write!(f, "upsert ")?;
        }

        write!(
            f,
            "{}@{}:",
            interner.resolve(&self.var.symbol),
            interner.resolve(&self.binding.symbol)
        )?;
        self.table.codegen(f, interner)?;
        write!(f, " ")?;
        match &self.data {
            CreationData::Object(obj) => obj.codegen(f, interner)?,
            CreationData::Array(arr) => arr.codegen(f, interner)?,
        }
        if let Some(conflict) = &self.on_conflict {
            write!(f, " on conflict (")?;
            for (index, field) in conflict.fields.iter().enumerate() {
                if index > 0 {
                    write!(f, ", ")?;
                }
                field.codegen(f, interner)?;
            }
            let action = match conflict.action {
                crate::query::ConflictAction::Replace => "replace",
                crate::query::ConflictAction::Merge => "merge",
                crate::query::ConflictAction::Ignore => "ignore",
            };
            write!(f, ") {action}")?;
        }
        if !self.links.is_empty() {
            write!(f, " link ")?;
            for (i, path) in self.links.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                path.codegen(f, interner)?;
            }
        }
        if let Some(tail) = &self.return_ {
            write!(f, "; ")?;
            tail.codegen(f, interner)?;
            write!(f, " }}")?;
        }
        Ok(())
    }
}

impl Codegen for Setter {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        self.path.codegen(f, interner)?;
        let op = match self.op {
            crate::SetterOp::Assign => "=",
            crate::SetterOp::Increment => "+=",
            crate::SetterOp::Decrement => "-=",
        };
        write!(f, " {op} ")?;
        self.value.codegen(f, interner)
    }
}

impl Codegen for UpdateQ {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "update ")?;
        self.var.codegen(f, interner)?;
        write!(f, "@")?;
        self.binding.codegen(f, interner)?;
        write!(f, ": ")?;
        self.table.codegen(f, interner)?;
        write!(f, " ")?;
        match &self.mutation {
            UpdateMutation::Merge(obj) => obj.codegen(f, interner)?,
            UpdateMutation::Set(setters) => {
                write!(f, "set {{")?;
                for (i, setter) in setters.iter().enumerate() {
                    if i > 0 {
                        write!(f, "; ")?;
                    }
                    setter.codegen(f, interner)?;
                }
                write!(f, "}}")?;
            }
        }
        if !self.links.is_empty() {
            write!(f, " link ")?;
            for (i, path) in self.links.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                path.codegen(f, interner)?;
            }
        }
        if let Some(condition) = &self.condition {
            write!(f, " where ")?;
            condition.codegen(f, interner)?;
        }
        if let Some(return_) = &self.return_ {
            write!(f, " return ")?;
            return_.codegen(f, interner)?;
        }
        Ok(())
    }
}

impl Codegen for LinkPath {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        self.start.codegen(f, interner)?;
        // Assuming LinkSegment has Codegen
        for _segment in &self.segments {
            // segment.codegen(f, interner)?;
            write!(f, " -> /* link segment */")?;
        }
        Ok(())
    }
}

impl Codegen for UnlinkQ {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "unlink ")?;
        for (i, path) in self.paths.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            path.codegen(f, interner)?;
        }
        if let Some(return_) = &self.return_ {
            write!(f, " return ")?;
            return_.codegen(f, interner)?;
        }
        Ok(())
    }
}

impl Codegen for DeleteQ {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "delete ")?;
        write!(
            f,
            "{}@{}: ",
            interner.resolve(&self.var.symbol),
            interner.resolve(&self.binding.symbol)
        )?;
        self.table.codegen(f, interner)?;
        if let Some(condition) = &self.condition {
            write!(f, " where ")?;
            condition.codegen(f, interner)?;
        }
        if let Some(return_) = &self.return_ {
            write!(f, " return ")?;
            return_.codegen(f, interner)?;
        }
        Ok(())
    }
}

impl Codegen for OrderByPart {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        self.field.codegen(f, interner)?;
        if self.direction == SortDirection::Desc {
            write!(f, " desc")?;
        }
        Ok(())
    }
}

impl Codegen for Range {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "range ")?;
        if let Some(start) = &self.start {
            start.codegen(f, interner)?;
        }
        if self.inclusive {
            write!(f, "..=")?;
        } else {
            write!(f, "..")?;
        }
        if let Some(end) = &self.end {
            end.codegen(f, interner)?;
        }
        Ok(())
    }
}

impl Codegen for Modifiers {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        if let Some(filter) = &self.filter {
            write!(f, "where ")?;
            filter.codegen(f, interner)?;
        }
        if let Some(order) = &self.order {
            if self.filter.is_some() {
                write!(f, " ")?;
            }
            write!(f, "order by ")?;
            for (i, part) in order.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                part.codegen(f, interner)?;
            }
        }
        if let Some(range) = &self.range {
            if self.filter.is_some() || self.order.is_some() {
                write!(f, " ")?;
            }
            range.codegen(f, interner)?;
        }
        Ok(())
    }
}

impl Codegen for Node {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "(")?;
        if let Some(var) = &self.var {
            write!(f, "{}", interner.resolve(&var.symbol))?;
        }
        if let Some(bind) = &self.bind {
            write!(f, "@{}", interner.resolve(&bind.symbol))?;
        }
        if self.var.is_some() || self.bind.is_some() {
            if self.ty.is_some() {
                write!(f, ": ")?;
            }
        } else if self.ty.is_some() {
            // No var or bind, but has type
        }
        if let Some(ty) = &self.ty {
            ty.codegen(f, interner)?;
        }
        if self.modifiers.filter.is_some()
            || self.modifiers.order.is_some()
            || self.modifiers.range.is_some()
        {
            write!(f, " ")?;
            self.modifiers.codegen(f, interner)?;
        }
        write!(f, ")")
    }
}
