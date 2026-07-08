//! Built-in XSD and GML type definitions.
//!
//! This module provides pre-defined type definitions for:
//! - XSD primitive types (string, integer, double, etc.)
//! - XSD derived types (normalizedString, token, NCName, etc.)
//! - GML types commonly used in PLATEAU CityGML (CodeType, MeasureType, etc.)
//! - GML geometry types (Point, LineString, Polygon, etc.)

use crate::schema::types::{
    AttributeDef, ComplexType, ContentModel, ElementDef, SimpleType, TypeDef, WhiteSpace,
};

/// XSD namespace URI.
pub const XS_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema";

/// GML namespace URI (GML 3.2).
pub const GML_NAMESPACE: &str = "http://www.opengis.net/gml/3.2";

/// GML namespace URI (GML 3.1).
pub const GML31_NAMESPACE: &str = "http://www.opengis.net/gml";

/// Local names of the built-in XSD datatypes fastxml recognizes (XSD 1.0
/// plus the XSD 1.1 datatypes fastxml supports).
pub(crate) const XSD_BUILTIN_TYPE_LOCALS: &[&str] = &[
    "anyType",
    "anySimpleType",
    "string",
    "boolean",
    "decimal",
    "float",
    "double",
    "duration",
    "dateTime",
    "time",
    "date",
    "gYearMonth",
    "gYear",
    "gMonthDay",
    "gDay",
    "gMonth",
    "hexBinary",
    "base64Binary",
    "anyURI",
    "QName",
    "NOTATION",
    "normalizedString",
    "token",
    "language",
    "NMTOKEN",
    "NMTOKENS",
    "Name",
    "NCName",
    "ID",
    "IDREF",
    "IDREFS",
    "ENTITY",
    "ENTITIES",
    "integer",
    "nonPositiveInteger",
    "negativeInteger",
    "long",
    "int",
    "short",
    "byte",
    "nonNegativeInteger",
    "unsignedLong",
    "unsignedInt",
    "unsignedShort",
    "unsignedByte",
    "positiveInteger",
    // XSD 1.1 datatypes fastxml supports.
    "anyAtomicType",
    "dateTimeStamp",
    "dayTimeDuration",
    "yearMonthDuration",
];

/// Whether `local` is the local name of a built-in XSD datatype.
pub(crate) fn is_builtin_xsd_type_local(local: &str) -> bool {
    XSD_BUILTIN_TYPE_LOCALS.contains(&local)
}

/// XSD primitive type names.
#[allow(missing_docs)]
pub mod xs {
    pub const STRING: &str = "xs:string";
    pub const BOOLEAN: &str = "xs:boolean";
    pub const DECIMAL: &str = "xs:decimal";
    pub const FLOAT: &str = "xs:float";
    pub const DOUBLE: &str = "xs:double";
    pub const DURATION: &str = "xs:duration";
    pub const DATE_TIME: &str = "xs:dateTime";
    pub const TIME: &str = "xs:time";
    pub const DATE: &str = "xs:date";
    pub const G_YEAR_MONTH: &str = "xs:gYearMonth";
    pub const G_YEAR: &str = "xs:gYear";
    pub const G_MONTH_DAY: &str = "xs:gMonthDay";
    pub const G_DAY: &str = "xs:gDay";
    pub const G_MONTH: &str = "xs:gMonth";
    pub const HEX_BINARY: &str = "xs:hexBinary";
    pub const BASE64_BINARY: &str = "xs:base64Binary";
    pub const ANY_URI: &str = "xs:anyURI";
    pub const QNAME: &str = "xs:QName";
    pub const NOTATION: &str = "xs:NOTATION";

    // Derived types
    pub const NORMALIZED_STRING: &str = "xs:normalizedString";
    pub const TOKEN: &str = "xs:token";
    pub const LANGUAGE: &str = "xs:language";
    pub const NMTOKEN: &str = "xs:NMTOKEN";
    pub const NMTOKENS: &str = "xs:NMTOKENS";
    pub const NAME: &str = "xs:Name";
    pub const NCNAME: &str = "xs:NCName";
    pub const ID: &str = "xs:ID";
    pub const IDREF: &str = "xs:IDREF";
    pub const IDREFS: &str = "xs:IDREFS";
    pub const ENTITY: &str = "xs:ENTITY";
    pub const ENTITIES: &str = "xs:ENTITIES";
    pub const INTEGER: &str = "xs:integer";
    pub const NON_POSITIVE_INTEGER: &str = "xs:nonPositiveInteger";
    pub const NEGATIVE_INTEGER: &str = "xs:negativeInteger";
    pub const LONG: &str = "xs:long";
    pub const INT: &str = "xs:int";
    pub const SHORT: &str = "xs:short";
    pub const BYTE: &str = "xs:byte";
    pub const NON_NEGATIVE_INTEGER: &str = "xs:nonNegativeInteger";
    pub const UNSIGNED_LONG: &str = "xs:unsignedLong";
    pub const UNSIGNED_INT: &str = "xs:unsignedInt";
    pub const UNSIGNED_SHORT: &str = "xs:unsignedShort";
    pub const UNSIGNED_BYTE: &str = "xs:unsignedByte";
    pub const POSITIVE_INTEGER: &str = "xs:positiveInteger";

    // Any type
    pub const ANY_TYPE: &str = "xs:anyType";
    pub const ANY_SIMPLE_TYPE: &str = "xs:anySimpleType";
}

/// GML type names.
#[allow(missing_docs)]
pub mod gml {
    pub const CODE_TYPE: &str = "gml:CodeType";
    pub const CODE_WITH_AUTHORITY_TYPE: &str = "gml:CodeWithAuthorityType";
    pub const MEASURE_TYPE: &str = "gml:MeasureType";
    pub const LENGTH_TYPE: &str = "gml:LengthType";
    pub const ANGLE_TYPE: &str = "gml:AngleType";
    pub const AREA_TYPE: &str = "gml:AreaType";
    pub const VOLUME_TYPE: &str = "gml:VolumeType";
    pub const SPEED_TYPE: &str = "gml:SpeedType";
    pub const SCALE_TYPE: &str = "gml:ScaleType";

    // Geometry types
    pub const POINT_TYPE: &str = "gml:PointType";
    pub const LINE_STRING_TYPE: &str = "gml:LineStringType";
    pub const LINEAR_RING_TYPE: &str = "gml:LinearRingType";
    pub const POLYGON_TYPE: &str = "gml:PolygonType";
    pub const MULTI_POINT_TYPE: &str = "gml:MultiPointType";
    pub const MULTI_CURVE_TYPE: &str = "gml:MultiCurveType";
    pub const MULTI_SURFACE_TYPE: &str = "gml:MultiSurfaceType";
    pub const MULTI_GEOMETRY_TYPE: &str = "gml:MultiGeometryType";
    pub const SOLID_TYPE: &str = "gml:SolidType";
    pub const COMPOSITE_SOLID_TYPE: &str = "gml:CompositeSolidType";
    pub const MULTI_SOLID_TYPE: &str = "gml:MultiSolidType";

    // Property types
    pub const POINT_PROPERTY_TYPE: &str = "gml:PointPropertyType";
    pub const CURVE_PROPERTY_TYPE: &str = "gml:CurvePropertyType";
    pub const SURFACE_PROPERTY_TYPE: &str = "gml:SurfacePropertyType";
    pub const SOLID_PROPERTY_TYPE: &str = "gml:SolidPropertyType";
    pub const GEOMETRY_PROPERTY_TYPE: &str = "gml:GeometryPropertyType";
    pub const MULTI_SURFACE_PROPERTY_TYPE: &str = "gml:MultiSurfacePropertyType";
    pub const MULTI_CURVE_PROPERTY_TYPE: &str = "gml:MultiCurvePropertyType";

    // Reference types
    pub const REFERENCE_TYPE: &str = "gml:ReferenceType";

    // Envelope
    pub const ENVELOPE_TYPE: &str = "gml:EnvelopeType";
    pub const BOUNDING_SHAPE_TYPE: &str = "gml:BoundingShapeType";

    // Abstract types
    pub const ABSTRACT_GML_TYPE: &str = "gml:AbstractGMLType";
    pub const ABSTRACT_FEATURE_TYPE: &str = "gml:AbstractFeatureType";
    pub const ABSTRACT_GEOMETRY_TYPE: &str = "gml:AbstractGeometryType";
    pub const ABSTRACT_SOLID_TYPE: &str = "gml:AbstractSolidType";
}

/// Creates a built-in list simple type (NMTOKENS, IDREFS, ENTITIES).
fn list_type(name: &str, item: &str) -> SimpleType {
    let mut st = SimpleType::new(name);
    st.item_type = Some(item.to_string());
    st.base_type = Some(format!("list({})", item));
    st
}

/// Creates XSD primitive type definitions.
pub fn create_xsd_primitive_types() -> Vec<(String, TypeDef)> {
    vec![
        // String types
        (
            xs::STRING.to_string(),
            TypeDef::Simple(SimpleType::new("string")),
        ),
        (
            xs::NORMALIZED_STRING.to_string(),
            TypeDef::Simple(
                SimpleType::new("normalizedString")
                    .with_base(xs::STRING)
                    .with_white_space(WhiteSpace::Replace),
            ),
        ),
        (
            xs::TOKEN.to_string(),
            TypeDef::Simple(
                SimpleType::new("token")
                    .with_base(xs::NORMALIZED_STRING)
                    .with_white_space(WhiteSpace::Collapse),
            ),
        ),
        (
            xs::LANGUAGE.to_string(),
            TypeDef::Simple(SimpleType::new("language").with_base(xs::TOKEN)),
        ),
        (
            xs::NMTOKEN.to_string(),
            TypeDef::Simple(SimpleType::new("NMTOKEN").with_base(xs::TOKEN)),
        ),
        (
            xs::NAME.to_string(),
            TypeDef::Simple(SimpleType::new("Name").with_base(xs::TOKEN)),
        ),
        (
            xs::NCNAME.to_string(),
            TypeDef::Simple(SimpleType::new("NCName").with_base(xs::NAME)),
        ),
        (
            xs::ID.to_string(),
            TypeDef::Simple(SimpleType::new("ID").with_base(xs::NCNAME)),
        ),
        (
            xs::IDREF.to_string(),
            TypeDef::Simple(SimpleType::new("IDREF").with_base(xs::NCNAME)),
        ),
        (
            xs::ENTITY.to_string(),
            TypeDef::Simple(SimpleType::new("ENTITY").with_base(xs::NCNAME)),
        ),
        (
            xs::NOTATION.to_string(),
            TypeDef::Simple(SimpleType::new("NOTATION")),
        ),
        // XSD 1.1 datatypes
        (
            "xs:dateTimeStamp".to_string(),
            TypeDef::Simple(SimpleType::new("dateTimeStamp").with_base(xs::DATE_TIME)),
        ),
        (
            "xs:dayTimeDuration".to_string(),
            TypeDef::Simple(SimpleType::new("dayTimeDuration").with_base(xs::DURATION)),
        ),
        (
            "xs:yearMonthDuration".to_string(),
            TypeDef::Simple(SimpleType::new("yearMonthDuration").with_base(xs::DURATION)),
        ),
        // Built-in list types
        (
            xs::NMTOKENS.to_string(),
            TypeDef::Simple(list_type("NMTOKENS", xs::NMTOKEN)),
        ),
        (
            xs::IDREFS.to_string(),
            TypeDef::Simple(list_type("IDREFS", xs::IDREF)),
        ),
        (
            xs::ENTITIES.to_string(),
            TypeDef::Simple(list_type("ENTITIES", xs::ENTITY)),
        ),
        // Boolean
        (
            xs::BOOLEAN.to_string(),
            TypeDef::Simple(SimpleType::new("boolean")),
        ),
        // Numeric types
        (
            xs::DECIMAL.to_string(),
            TypeDef::Simple(SimpleType::new("decimal")),
        ),
        (
            xs::FLOAT.to_string(),
            TypeDef::Simple(SimpleType::new("float")),
        ),
        (
            xs::DOUBLE.to_string(),
            TypeDef::Simple(SimpleType::new("double")),
        ),
        (
            xs::INTEGER.to_string(),
            TypeDef::Simple(SimpleType::new("integer").with_base(xs::DECIMAL)),
        ),
        (
            xs::LONG.to_string(),
            TypeDef::Simple(SimpleType::new("long").with_base(xs::INTEGER)),
        ),
        (
            xs::INT.to_string(),
            TypeDef::Simple(SimpleType::new("int").with_base(xs::LONG)),
        ),
        (
            xs::SHORT.to_string(),
            TypeDef::Simple(SimpleType::new("short").with_base(xs::INT)),
        ),
        (
            xs::BYTE.to_string(),
            TypeDef::Simple(SimpleType::new("byte").with_base(xs::SHORT)),
        ),
        (
            xs::NON_NEGATIVE_INTEGER.to_string(),
            TypeDef::Simple(SimpleType::new("nonNegativeInteger").with_base(xs::INTEGER)),
        ),
        (
            xs::POSITIVE_INTEGER.to_string(),
            TypeDef::Simple(SimpleType::new("positiveInteger").with_base(xs::NON_NEGATIVE_INTEGER)),
        ),
        (
            xs::UNSIGNED_LONG.to_string(),
            TypeDef::Simple(SimpleType::new("unsignedLong").with_base(xs::NON_NEGATIVE_INTEGER)),
        ),
        (
            xs::UNSIGNED_INT.to_string(),
            TypeDef::Simple(SimpleType::new("unsignedInt").with_base(xs::UNSIGNED_LONG)),
        ),
        (
            xs::UNSIGNED_SHORT.to_string(),
            TypeDef::Simple(SimpleType::new("unsignedShort").with_base(xs::UNSIGNED_INT)),
        ),
        (
            xs::UNSIGNED_BYTE.to_string(),
            TypeDef::Simple(SimpleType::new("unsignedByte").with_base(xs::UNSIGNED_SHORT)),
        ),
        (
            xs::NON_POSITIVE_INTEGER.to_string(),
            TypeDef::Simple(SimpleType::new("nonPositiveInteger").with_base(xs::INTEGER)),
        ),
        (
            xs::NEGATIVE_INTEGER.to_string(),
            TypeDef::Simple(SimpleType::new("negativeInteger").with_base(xs::NON_POSITIVE_INTEGER)),
        ),
        // Date/time types
        (
            xs::DATE_TIME.to_string(),
            TypeDef::Simple(SimpleType::new("dateTime")),
        ),
        (
            xs::DATE.to_string(),
            TypeDef::Simple(SimpleType::new("date")),
        ),
        (
            xs::TIME.to_string(),
            TypeDef::Simple(SimpleType::new("time")),
        ),
        (
            xs::DURATION.to_string(),
            TypeDef::Simple(SimpleType::new("duration")),
        ),
        (
            xs::G_YEAR.to_string(),
            TypeDef::Simple(SimpleType::new("gYear")),
        ),
        (
            xs::G_YEAR_MONTH.to_string(),
            TypeDef::Simple(SimpleType::new("gYearMonth")),
        ),
        (
            xs::G_MONTH.to_string(),
            TypeDef::Simple(SimpleType::new("gMonth")),
        ),
        (
            xs::G_MONTH_DAY.to_string(),
            TypeDef::Simple(SimpleType::new("gMonthDay")),
        ),
        (
            xs::G_DAY.to_string(),
            TypeDef::Simple(SimpleType::new("gDay")),
        ),
        // Binary types
        (
            xs::HEX_BINARY.to_string(),
            TypeDef::Simple(SimpleType::new("hexBinary")),
        ),
        (
            xs::BASE64_BINARY.to_string(),
            TypeDef::Simple(SimpleType::new("base64Binary")),
        ),
        // URI and QName
        (
            xs::ANY_URI.to_string(),
            TypeDef::Simple(SimpleType::new("anyURI")),
        ),
        (
            xs::QNAME.to_string(),
            TypeDef::Simple(SimpleType::new("QName")),
        ),
        // Any types
        (
            xs::ANY_TYPE.to_string(),
            TypeDef::Complex(ComplexType::new("anyType")),
        ),
        (
            xs::ANY_SIMPLE_TYPE.to_string(),
            TypeDef::Simple(SimpleType::new("anySimpleType")),
        ),
    ]
}

/// Creates GML type definitions commonly used in CityGML/PLATEAU.
pub fn create_gml_types() -> Vec<(String, TypeDef)> {
    vec![
        // CodeType - text with codeSpace attribute
        (
            gml::CODE_TYPE.to_string(),
            TypeDef::Complex(create_code_type()),
        ),
        (
            gml::CODE_WITH_AUTHORITY_TYPE.to_string(),
            TypeDef::Complex(create_code_with_authority_type()),
        ),
        // Measure types - numeric value with uom attribute
        (
            gml::MEASURE_TYPE.to_string(),
            TypeDef::Complex(create_measure_type()),
        ),
        (
            gml::LENGTH_TYPE.to_string(),
            TypeDef::Complex(create_measure_type_variant("LengthType")),
        ),
        (
            gml::ANGLE_TYPE.to_string(),
            TypeDef::Complex(create_measure_type_variant("AngleType")),
        ),
        (
            gml::AREA_TYPE.to_string(),
            TypeDef::Complex(create_measure_type_variant("AreaType")),
        ),
        (
            gml::VOLUME_TYPE.to_string(),
            TypeDef::Complex(create_measure_type_variant("VolumeType")),
        ),
        (
            gml::SPEED_TYPE.to_string(),
            TypeDef::Complex(create_measure_type_variant("SpeedType")),
        ),
        (
            gml::SCALE_TYPE.to_string(),
            TypeDef::Complex(create_measure_type_variant("ScaleType")),
        ),
        // Reference type
        (
            gml::REFERENCE_TYPE.to_string(),
            TypeDef::Complex(create_reference_type()),
        ),
        // Abstract types
        (
            gml::ABSTRACT_GML_TYPE.to_string(),
            TypeDef::Complex(create_abstract_gml_type()),
        ),
        (
            gml::ABSTRACT_FEATURE_TYPE.to_string(),
            TypeDef::Complex(create_abstract_feature_type()),
        ),
        (
            gml::ABSTRACT_GEOMETRY_TYPE.to_string(),
            TypeDef::Complex(create_abstract_geometry_type()),
        ),
        (
            gml::ABSTRACT_SOLID_TYPE.to_string(),
            TypeDef::Complex(create_abstract_solid_type()),
        ),
        // Geometry types
        (
            gml::POINT_TYPE.to_string(),
            TypeDef::Complex(create_geometry_type("PointType")),
        ),
        (
            gml::LINE_STRING_TYPE.to_string(),
            TypeDef::Complex(create_geometry_type("LineStringType")),
        ),
        (
            gml::LINEAR_RING_TYPE.to_string(),
            TypeDef::Complex(create_geometry_type("LinearRingType")),
        ),
        (
            gml::POLYGON_TYPE.to_string(),
            TypeDef::Complex(create_geometry_type("PolygonType")),
        ),
        (
            gml::MULTI_POINT_TYPE.to_string(),
            TypeDef::Complex(create_geometry_type("MultiPointType")),
        ),
        (
            gml::MULTI_CURVE_TYPE.to_string(),
            TypeDef::Complex(create_geometry_type("MultiCurveType")),
        ),
        (
            gml::MULTI_SURFACE_TYPE.to_string(),
            TypeDef::Complex(create_geometry_type("MultiSurfaceType")),
        ),
        (
            gml::SOLID_TYPE.to_string(),
            TypeDef::Complex(create_solid_type()),
        ),
        (
            gml::COMPOSITE_SOLID_TYPE.to_string(),
            TypeDef::Complex(create_geometry_type("CompositeSolidType")),
        ),
        (
            gml::MULTI_SOLID_TYPE.to_string(),
            TypeDef::Complex(create_geometry_type("MultiSolidType")),
        ),
        // Property types
        (
            gml::GEOMETRY_PROPERTY_TYPE.to_string(),
            TypeDef::Complex(create_property_type("GeometryPropertyType")),
        ),
        (
            gml::POINT_PROPERTY_TYPE.to_string(),
            TypeDef::Complex(create_property_type("PointPropertyType")),
        ),
        (
            gml::CURVE_PROPERTY_TYPE.to_string(),
            TypeDef::Complex(create_property_type("CurvePropertyType")),
        ),
        (
            gml::SURFACE_PROPERTY_TYPE.to_string(),
            TypeDef::Complex(create_property_type("SurfacePropertyType")),
        ),
        (
            gml::SOLID_PROPERTY_TYPE.to_string(),
            TypeDef::Complex(create_property_type("SolidPropertyType")),
        ),
        (
            gml::MULTI_SURFACE_PROPERTY_TYPE.to_string(),
            TypeDef::Complex(create_property_type("MultiSurfacePropertyType")),
        ),
        (
            gml::MULTI_CURVE_PROPERTY_TYPE.to_string(),
            TypeDef::Complex(create_property_type("MultiCurvePropertyType")),
        ),
        // Envelope
        (
            gml::ENVELOPE_TYPE.to_string(),
            TypeDef::Complex(create_envelope_type()),
        ),
        (
            gml::BOUNDING_SHAPE_TYPE.to_string(),
            TypeDef::Complex(create_bounding_shape_type()),
        ),
    ]
}

fn create_code_type() -> ComplexType {
    let mut ct = ComplexType::new("CodeType");
    ct.content = ContentModel::SimpleContent {
        base_type: xs::STRING.to_string(),
    };
    ct.attributes
        .push(AttributeDef::new("codeSpace").with_type(xs::ANY_URI));
    ct
}

fn create_code_with_authority_type() -> ComplexType {
    let mut ct = ComplexType::new("CodeWithAuthorityType");
    ct.content = ContentModel::SimpleContent {
        base_type: xs::STRING.to_string(),
    };
    ct.attributes
        .push(AttributeDef::required("codeSpace").with_type(xs::ANY_URI));
    ct
}

fn create_measure_type() -> ComplexType {
    let mut ct = ComplexType::new("MeasureType");
    ct.content = ContentModel::SimpleContent {
        base_type: xs::DOUBLE.to_string(),
    };
    ct.attributes
        .push(AttributeDef::required("uom").with_type(xs::ANY_URI));
    ct
}

fn create_measure_type_variant(name: &str) -> ComplexType {
    let mut ct = ComplexType::new(name);
    // These types use simpleContent/restriction on MeasureType
    // so they should be SimpleContent, not ComplexExtension
    ct.content = ContentModel::SimpleContent {
        base_type: gml::MEASURE_TYPE.to_string(),
    };
    ct
}

fn create_reference_type() -> ComplexType {
    let mut ct = ComplexType::new("ReferenceType");
    ct.content = ContentModel::Empty;
    ct.attributes
        .push(AttributeDef::new("href").with_type(xs::ANY_URI));
    ct.attributes
        .push(AttributeDef::new("title").with_type(xs::STRING));
    ct.attributes
        .push(AttributeDef::new("role").with_type(xs::ANY_URI));
    ct.attributes
        .push(AttributeDef::new("nilReason").with_type(xs::STRING));
    ct
}

fn create_abstract_gml_type() -> ComplexType {
    let mut ct = ComplexType::new("AbstractGMLType");
    ct.is_abstract = true;
    ct.content = ContentModel::Sequence(vec![
        ElementDef::new("description")
            .with_type(xs::STRING)
            .optional(),
        ElementDef::new("name")
            .with_type(gml::CODE_TYPE)
            .optional()
            .unbounded(),
    ]);
    ct.attributes
        .push(AttributeDef::new("id").with_type(xs::ID));
    ct
}

fn create_abstract_feature_type() -> ComplexType {
    let mut ct = ComplexType::new("AbstractFeatureType");
    ct.is_abstract = true;
    ct.content = ContentModel::ComplexExtension {
        base_type: gml::ABSTRACT_GML_TYPE.to_string(),
        elements: vec![
            ElementDef::new("boundedBy")
                .with_type(gml::BOUNDING_SHAPE_TYPE)
                .optional(),
        ],
    };
    ct
}

fn create_abstract_geometry_type() -> ComplexType {
    let mut ct = ComplexType::new("AbstractGeometryType");
    ct.is_abstract = true;
    ct.content = ContentModel::ComplexExtension {
        base_type: gml::ABSTRACT_GML_TYPE.to_string(),
        elements: Vec::new(),
    };
    ct.attributes
        .push(AttributeDef::new("srsName").with_type(xs::ANY_URI));
    ct.attributes
        .push(AttributeDef::new("srsDimension").with_type(xs::POSITIVE_INTEGER));
    ct
}

fn create_abstract_solid_type() -> ComplexType {
    let mut ct = ComplexType::new("AbstractSolidType");
    ct.is_abstract = true;
    ct.content = ContentModel::ComplexExtension {
        base_type: gml::ABSTRACT_GEOMETRY_TYPE.to_string(),
        elements: Vec::new(),
    };
    ct
}

fn create_geometry_type(name: &str) -> ComplexType {
    let mut ct = ComplexType::new(name);
    ct.content = ContentModel::ComplexExtension {
        base_type: gml::ABSTRACT_GEOMETRY_TYPE.to_string(),
        elements: Vec::new(),
    };
    ct
}

/// Creates SolidType with exterior and interior elements.
/// SolidType extends AbstractSolidType (not AbstractGeometryType directly)
/// and has exterior (SurfacePropertyType) and interior (SurfacePropertyType) elements.
fn create_solid_type() -> ComplexType {
    let mut ct = ComplexType::new("SolidType");
    ct.content = ContentModel::ComplexExtension {
        base_type: gml::ABSTRACT_SOLID_TYPE.to_string(),
        elements: vec![
            ElementDef::new("exterior")
                .with_type(gml::SURFACE_PROPERTY_TYPE)
                .optional(),
            ElementDef::new("interior")
                .with_type(gml::SURFACE_PROPERTY_TYPE)
                .optional()
                .unbounded(),
        ],
    };
    ct
}

fn create_property_type(name: &str) -> ComplexType {
    let mut ct = ComplexType::new(name);
    ct.content = ContentModel::Empty;
    ct.attributes
        .push(AttributeDef::new("href").with_type(xs::ANY_URI));
    ct.attributes
        .push(AttributeDef::new("nilReason").with_type(xs::STRING));
    ct
}

fn create_envelope_type() -> ComplexType {
    let mut ct = ComplexType::new("EnvelopeType");
    ct.content = ContentModel::Sequence(vec![
        ElementDef::new("lowerCorner").with_type(xs::STRING),
        ElementDef::new("upperCorner").with_type(xs::STRING),
    ]);
    ct.attributes
        .push(AttributeDef::new("srsName").with_type(xs::ANY_URI));
    ct.attributes
        .push(AttributeDef::new("srsDimension").with_type(xs::POSITIVE_INTEGER));
    ct
}

fn create_bounding_shape_type() -> ComplexType {
    let mut ct = ComplexType::new("BoundingShapeType");
    ct.content = ContentModel::Choice(vec![
        ElementDef::new("Envelope").with_type(gml::ENVELOPE_TYPE),
        ElementDef::new("Null").with_type(xs::STRING),
    ]);
    ct
}

/// Registers all built-in types into a CompiledSchema.
///
/// XSD primitives live under the XSD namespace; the GML convenience types
/// are registered under BOTH the GML 3.1 and GML 3.2 namespace URIs (they
/// share local names across versions). Schema-provided definitions are
/// never clobbered (`or_insert`).
pub fn register_builtin_types(schema: &mut crate::schema::types::CompiledSchema) {
    use crate::schema::types::NsName;

    for (name, type_def) in create_xsd_primitive_types() {
        if let Some(local) = name.strip_prefix("xs:") {
            schema
                .types_ns
                .entry(NsName::new(XS_NAMESPACE, local))
                .or_insert(type_def);
        }
    }

    for (name, type_def) in create_gml_types() {
        if let Some(local) = name.strip_prefix("gml:") {
            schema
                .types_ns
                .entry(NsName::new(GML_NAMESPACE, local))
                .or_insert_with(|| type_def.clone());
            schema
                .types_ns
                .entry(NsName::new(GML31_NAMESPACE, local))
                .or_insert(type_def);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_xsd_primitives() {
        let types = create_xsd_primitive_types();
        assert!(types.iter().any(|(n, _)| n == xs::STRING));
        assert!(types.iter().any(|(n, _)| n == xs::INTEGER));
        assert!(types.iter().any(|(n, _)| n == xs::DOUBLE));
        assert!(types.iter().any(|(n, _)| n == xs::DATE_TIME));
    }

    #[test]
    fn test_create_gml_types() {
        let types = create_gml_types();
        assert!(types.iter().any(|(n, _)| n == gml::CODE_TYPE));
        assert!(types.iter().any(|(n, _)| n == gml::MEASURE_TYPE));
        assert!(types.iter().any(|(n, _)| n == gml::POINT_TYPE));
    }

    #[test]
    fn test_register_builtin_types() {
        let mut schema = crate::schema::types::CompiledSchema::new();
        register_builtin_types(&mut schema);

        // Should have xs: prefixed
        assert!(schema.get_type("xs:string").is_some());
        assert!(schema.get_type("xs:integer").is_some());

        // Should also have unprefixed
        assert!(schema.get_type("string").is_some());
        assert!(schema.get_type("integer").is_some());

        // GML types
        assert!(schema.get_type("gml:CodeType").is_some());
        assert!(schema.get_type("CodeType").is_some());
    }

    #[test]
    fn test_code_type_structure() {
        let ct = create_code_type();
        assert_eq!(ct.name, "CodeType");
        assert!(matches!(ct.content, ContentModel::SimpleContent { .. }));
        assert!(ct.attributes.iter().any(|a| a.name == "codeSpace"));
    }

    #[test]
    fn test_measure_type_structure() {
        let ct = create_measure_type();
        assert_eq!(ct.name, "MeasureType");
        let uom_attr = ct.attributes.iter().find(|a| a.name == "uom").unwrap();
        assert!(uom_attr.required);
    }
}
