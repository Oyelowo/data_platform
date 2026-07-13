use crate::query::{Edge, EdgeDirection, FromNode, HopRange, LinkSegment, LinksMatchKind, SelectQ};
use crate::{Codegen, Interner};
use std::fmt::{self, Write};

impl Codegen for SelectQ {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "select ")?;
        self.projection.codegen(f, interner)?;

        debug_assert!(
            !self.from.is_empty(),
            "SELECT codegen expects a required FROM clause"
        );
        write!(f, " from ")?;
        for (i, from) in self.from.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            from.codegen(f, interner)?;
        }

        if !self.links.is_empty() {
            write!(f, " links")?;
            if matches!(self.links_match_kind, LinksMatchKind::Required) {
                write!(f, " inner")?;
            }
            write!(f, " ")?;
            for (i, link) in self.links.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                link.codegen(f, interner)?;
            }
        }

        if let Some(where_clause) = &self.where_clause {
            write!(f, " where ")?;
            where_clause.codegen(f, interner)?;
        }

        if let Some(group_by) = &self.group_by {
            write!(f, " group by ")?;
            for (i, key) in group_by.keys.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                if let Some(name) = &key.name {
                    write!(f, "{}: ", interner.resolve(&name.symbol))?;
                }
                key.expr.codegen(f, interner)?;
            }

            write!(f, " into {}", interner.resolve(&group_by.into.symbol))?;
        }

        if let Some(order_by) = &self.order_by {
            write!(f, " order by ")?;
            for (i, part) in order_by.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                part.codegen(f, interner)?;
            }
        }

        if let Some(range) = &self.range {
            write!(f, " ")?;
            range.codegen(f, interner)?;
        }

        Ok(())
    }
}

impl Codegen for FromNode {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        let has_mods = self.modifiers.filter.is_some()
            || self.modifiers.order.is_some()
            || self.modifiers.range.is_some();

        if has_mods {
            write!(f, "(")?;
        }
        if let Some(var) = &self.var {
            write!(f, "{}", interner.resolve(&var.symbol))?;
        }
        if let Some(bind) = &self.bind {
            write!(f, "@{}", interner.resolve(&bind.symbol))?;
        }
        if let Some(ty) = &self.ty {
            write!(f, ":")?;
            ty.codegen(f, interner)?;
        }
        if has_mods {
            write!(f, " ")?;
            self.modifiers.codegen(f, interner)?;
            write!(f, ")")
        } else {
            Ok(())
        }
    }
}

impl Codegen for Edge {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "[")?;
        if let Some(var) = &self.var {
            write!(f, "{}", interner.resolve(&var.symbol))?;
        }
        if let Some(bind) = &self.bind {
            write!(f, "@{}", interner.resolve(&bind.symbol))?;
        }
        if let Some(ty) = &self.ty {
            write!(f, ":")?;
            ty.codegen(f, interner)?;
        }
        if let Some(hops) = &self.hops {
            hops.codegen(f, interner)?;
        }
        self.modifiers.codegen(f, interner)?;
        write!(f, "]")
    }
}

impl Codegen for LinkSegment {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, " ")?;
        match self.edge.direction {
            EdgeDirection::Forward => write!(f, "->")?,
            EdgeDirection::Backward => write!(f, "<-")?,
            EdgeDirection::Bidirectional => write!(f, "<->")?,
        }
        write!(f, " ")?;
        self.edge.codegen(f, interner)?;
        write!(f, " ")?;
        self.target.codegen(f, interner)
    }
}

impl Codegen for HopRange {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        if let Some(start) = &self.start {
            start.codegen(f, interner)?;
        }
        write!(f, "..")?;
        if self.inclusive {
            write!(f, "=")?;
        }
        if let Some(end) = &self.end {
            end.codegen(f, interner)?;
        }
        Ok(())
    }
}
