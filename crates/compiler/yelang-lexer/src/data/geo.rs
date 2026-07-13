use crate::{
    Char, CharCursor, CharLexerResult, Either, FloatLexed, IntLexed, OneOf7, ParseChars, Repeat,
    SeparatedList, Span, SurroundedBy, Whitespace, word::*,
};
use std::fmt::{self, Display};

type Number = Either<FloatLexed, IntLexed>;

// A single coordinate pair: 10.0 20.0
type Coordinate = (Number, Repeat<Whitespace>, Number);

// A list of coordinates, e.g., (10 20, 30 40)
type CoordinateList =
    SurroundedBy<Char<'('>, SeparatedList<Coordinate, Char<','>, true>, Char<')'>>;

// A list of linestrings, e.g., ((10 20, 30 40), (50 60, 70 80))
type LineStringList =
    SurroundedBy<Char<'('>, SeparatedList<CoordinateList, Char<','>, true>, Char<')'>>;

// A list of polygons
type PolygonList =
    SurroundedBy<Char<'('>, SeparatedList<LineStringList, Char<','>, true>, Char<')'>>;

#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    Point(Span),
    LineString(Span),
    Polygon(Span),
    MultiPoint(Span),
    MultiLineString(Span),
    MultiPolygon(Span),
    GeometryCollection(Span),
}

impl Display for Geometry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "geometry")
    }
}

impl ParseChars for Geometry {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let checkpoint = cursor.checkpoint();
        cursor.parse::<Repeat<Whitespace>>().ok();

        type GeometryKeyword = OneOf7<
            Word5<'P', 'O', 'I', 'N', 'T'>,
            Word10<'L', 'I', 'N', 'E', 'S', 'T', 'R', 'I', 'N', 'G'>,
            Word7<'P', 'O', 'L', 'Y', 'G', 'O', 'N'>,
            Word10<'M', 'U', 'L', 'T', 'I', 'P', 'O', 'I', 'N', 'T'>,
            Word15<'M', 'U', 'L', 'T', 'I', 'L', 'I', 'N', 'E', 'S', 'T', 'R', 'I', 'N', 'G'>,
            Word12<'M', 'U', 'L', 'T', 'I', 'P', 'O', 'L', 'Y', 'G', 'O', 'N'>,
            Word18<
                'G',
                'E',
                'O',
                'M',
                'E',
                'T',
                'R',
                'Y',
                'C',
                'O',
                'L',
                'L',
                'E',
                'C',
                'T',
                'I',
                'O',
                'N',
            >,
        >;

        let keyword = cursor.parse::<GeometryKeyword>()?;
        cursor.parse::<Repeat<Whitespace>>().ok();

        match keyword {
            OneOf7::_1(_) => {
                // POINT
                cursor.parse::<SurroundedBy<Char<'('>, Coordinate, Char<')'>>>()?;
                Ok(Geometry::Point(cursor.span_since(checkpoint)))
            }
            OneOf7::_2(_) => {
                // LINESTRING
                cursor.parse::<CoordinateList>()?;
                Ok(Geometry::LineString(cursor.span_since(checkpoint)))
            }
            OneOf7::_3(_) => {
                // POLYGON
                cursor.parse::<LineStringList>()?;
                Ok(Geometry::Polygon(cursor.span_since(checkpoint)))
            }
            OneOf7::_4(_) => {
                // MULTIPOINT
                cursor.parse::<CoordinateList>()?;
                Ok(Geometry::MultiPoint(cursor.span_since(checkpoint)))
            }
            OneOf7::_5(_) => {
                // MULTILINESTRING
                cursor.parse::<LineStringList>()?;
                Ok(Geometry::MultiLineString(cursor.span_since(checkpoint)))
            }
            OneOf7::_6(_) => {
                // MULTIPOLYGON
                cursor.parse::<PolygonList>()?;
                Ok(Geometry::MultiPolygon(cursor.span_since(checkpoint)))
            }
            OneOf7::_7(_) => {
                // GEOMETRYCOLLECTION
                type Geometries = SeparatedList<Geometry, Char<','>, true>;
                cursor.parse::<SurroundedBy<Char<'('>, Geometries, Char<')'>>>()?;
                Ok(Geometry::GeometryCollection(cursor.span_since(checkpoint)))
            }
        }
    }
}
