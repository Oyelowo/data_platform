//! Table schema definition, Arrow conversion, and evolution rules.

use std::collections::HashMap;

use arrow_schema::{Field, Schema, SchemaRef};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::types::ColumnType;
use crate::{Error, Result};

/// Definition of a single column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnDef {
    /// Column name.
    pub name: String,
    /// Logical type.
    pub ty: ColumnType,
    /// Whether the column may contain nulls.
    pub nullable: bool,
}

/// In-memory representation of a table schema.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TableSchema {
    /// Ordered column definitions.
    pub columns: Vec<ColumnDef>,
}

impl TableSchema {
    /// Create an empty schema.
    pub fn empty() -> Self {
        Self {
            columns: Vec::new(),
        }
    }

    /// Infer a schema from a batch of column names, marking every column as
    /// nullable `Utf8`. This is the default first-ingest behaviour.
    pub fn infer_from_names(names: &[String]) -> Self {
        Self {
            columns: names
                .iter()
                .map(|name| ColumnDef {
                    name: name.clone(),
                    ty: ColumnType::Utf8,
                    nullable: true,
                })
                .collect(),
        }
    }

    /// Convert to an Arrow `SchemaRef`.
    pub fn to_arrow(&self) -> SchemaRef {
        let fields: Vec<Field> = self
            .columns
            .iter()
            .map(|col| Field::new(&col.name, col.ty.to_arrow(), col.nullable))
            .collect();
        SchemaRef::new(Schema::new(fields))
    }

    /// Build a `TableSchema` from an Arrow `Schema`.
    pub fn try_from_arrow(schema: &Schema) -> Result<Self> {
        let mut columns = Vec::with_capacity(schema.fields().len());
        for field in schema.fields() {
            columns.push(ColumnDef {
                name: field.name().clone(),
                ty: ColumnType::try_from_arrow(field.data_type())?,
                nullable: field.is_nullable(),
            });
        }
        Ok(Self { columns })
    }

    /// Look up a column by name.
    pub fn column(&self, name: &str) -> Option<&ColumnDef> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// Return the index of a column by name.
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }

    /// Validate an incoming batch against the current schema and, if necessary,
    /// evolve the schema by adding new nullable columns.
    ///
    /// Returns the validated/evolved schema and the row count of the batch.
    ///
    /// # Nullability enforcement
    ///
    /// * Columns declared `nullable: false` must be present in the batch and
    ///   must not contain `None` values.
    /// * New columns introduced by schema evolution are always nullable.
    pub fn validate_or_evolve(
        &self,
        batch: &[(String, Vec<Option<Bytes>>)],
    ) -> Result<(Self, usize)> {
        if batch.is_empty() {
            return Ok((self.clone(), 0));
        }

        // All present columns must have the same row count.
        let row_count = batch[0].1.len();
        for (name, values) in batch {
            if values.len() != row_count {
                return Err(Error::Batch(format!(
                    "column {name} has {} rows, expected {row_count}",
                    values.len()
                )));
            }
        }

        let mut new_schema = self.clone();
        let mut seen = std::collections::HashSet::new();

        for (name, values) in batch {
            if !seen.insert(name.clone()) {
                return Err(Error::Batch(format!("duplicate column in batch: {name}")));
            }

            if let Some(def) = new_schema.column(name) {
                if !def.nullable {
                    for (idx, v) in values.iter().enumerate() {
                        if v.is_none() {
                            return Err(Error::Batch(format!(
                                "non-nullable column '{name}' contains null at row {idx}"
                            )));
                        }
                    }
                }
            } else {
                // New columns are always nullable Utf8.
                new_schema.columns.push(ColumnDef {
                    name: name.clone(),
                    ty: ColumnType::Utf8,
                    nullable: true,
                });
            }
        }

        // Columns that are declared non-nullable must be present in the batch.
        for def in &self.columns {
            if !def.nullable && !seen.contains(&def.name) {
                return Err(Error::Batch(format!(
                    "non-nullable column '{}' is missing from batch",
                    def.name
                )));
            }
        }

        Ok((new_schema, row_count))
    }
}

/// Build a fast lookup from column name to typed definition.
pub fn column_map(schema: &TableSchema) -> HashMap<String, ColumnDef> {
    schema
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.clone()))
        .collect()
}
