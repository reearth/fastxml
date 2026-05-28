//! Value-space validation tests for built-in atomic types.
//!
//! Each section starts with the lexical-space rules taken from
//! XML Schema Part 2 (W3C XSD 1.0) and then exercises both canonical valid
//! literals and a representative sample of invalid lexical forms.

mod common;

// ===========================================================================
// xs:boolean
// ===========================================================================
// Per XSD 1.0 §3.2.2, the lexical space of `boolean` is exactly the four
// literals `true`, `false`, `1`, `0` (after the fixed `whiteSpace=collapse`
// normalization). Anything else — `True`, `yes`, `2`, ... — is invalid.

const XSD_BOOL_ELEMENT: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="value" type="xs:boolean"/>
</xs:schema>"#;

// Element whose anonymous simple type restricts `xs:boolean`. Exercises
// resolution of the built-in primitive through the base-type chain.
const XSD_BOOL_RESTRICTION: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="value">
    <xs:simpleType>
      <xs:restriction base="xs:boolean"/>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;

test_validation!(
    bool_true_valid,
    r#"<?xml version="1.0"?>
<value>true</value>"#,
    XSD_BOOL_ELEMENT,
    true
);
test_validation!(
    bool_false_valid,
    r#"<?xml version="1.0"?>
<value>false</value>"#,
    XSD_BOOL_ELEMENT,
    true
);
test_validation!(
    bool_one_valid,
    r#"<?xml version="1.0"?>
<value>1</value>"#,
    XSD_BOOL_ELEMENT,
    true
);
test_validation!(
    bool_zero_valid,
    r#"<?xml version="1.0"?>
<value>0</value>"#,
    XSD_BOOL_ELEMENT,
    true
);

// Valid literal through a restriction of xs:boolean (base-chain resolution).
test_validation!(
    bool_restriction_true_valid,
    r#"<?xml version="1.0"?>
<value>true</value>"#,
    XSD_BOOL_RESTRICTION,
    true
);

test_validation!(
    bool_capital_true_invalid,
    r#"<?xml version="1.0"?>
<value>True</value>"#,
    XSD_BOOL_ELEMENT,
    false
);
test_validation!(
    bool_yes_invalid,
    r#"<?xml version="1.0"?>
<value>yes</value>"#,
    XSD_BOOL_ELEMENT,
    false
);
test_validation!(
    bool_two_invalid,
    r#"<?xml version="1.0"?>
<value>2</value>"#,
    XSD_BOOL_ELEMENT,
    false
);

// ===========================================================================
// xs:integer and the integer family
// ===========================================================================
// `xs:integer` lexical (XSD 1.0 §3.3.13): `[+\-]?[0-9]+`. No decimal point,
// no exponent, arbitrary precision.
// `xs:int` adds the 32-bit signed range [-2147483648, 2147483647].
// `xs:nonNegativeInteger` adds value >= 0 (used e.g. for
// `bldg:storeysAboveGround` in CityGML 2.0 Building).
// `xs:positiveInteger` adds value >= 1.

const XSD_INTEGER: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="value" type="xs:integer"/>
</xs:schema>"#;

const XSD_INT: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="value" type="xs:int"/>
</xs:schema>"#;

const XSD_NON_NEGATIVE_INTEGER: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="value" type="xs:nonNegativeInteger"/>
</xs:schema>"#;

const XSD_POSITIVE_INTEGER: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="value" type="xs:positiveInteger"/>
</xs:schema>"#;

test_validation!(
    integer_zero_valid,
    r#"<?xml version="1.0"?>
<value>0</value>"#,
    XSD_INTEGER,
    true
);
test_validation!(
    integer_positive_valid,
    r#"<?xml version="1.0"?>
<value>42</value>"#,
    XSD_INTEGER,
    true
);
test_validation!(
    integer_negative_valid,
    r#"<?xml version="1.0"?>
<value>-42</value>"#,
    XSD_INTEGER,
    true
);
test_validation!(
    integer_explicit_plus_valid,
    r#"<?xml version="1.0"?>
<value>+42</value>"#,
    XSD_INTEGER,
    true
);

test_validation!(
    integer_decimal_invalid,
    r#"<?xml version="1.0"?>
<value>1.5</value>"#,
    XSD_INTEGER,
    false
);
test_validation!(
    integer_alpha_invalid,
    r#"<?xml version="1.0"?>
<value>abc</value>"#,
    XSD_INTEGER,
    false
);
test_validation!(
    integer_empty_invalid,
    r#"<?xml version="1.0"?>
<value></value>"#,
    XSD_INTEGER,
    false
);
test_validation!(
    integer_exponent_invalid,
    r#"<?xml version="1.0"?>
<value>1e2</value>"#,
    XSD_INTEGER,
    false
);

// xs:int — 32-bit signed range
test_validation!(
    int_min_boundary_valid,
    r#"<?xml version="1.0"?>
<value>-2147483648</value>"#,
    XSD_INT,
    true
);
test_validation!(
    int_max_boundary_valid,
    r#"<?xml version="1.0"?>
<value>2147483647</value>"#,
    XSD_INT,
    true
);
test_validation!(
    int_above_max_invalid,
    r#"<?xml version="1.0"?>
<value>2147483648</value>"#,
    XSD_INT,
    false
);
test_validation!(
    int_below_min_invalid,
    r#"<?xml version="1.0"?>
<value>-2147483649</value>"#,
    XSD_INT,
    false
);

// xs:nonNegativeInteger — e.g. bldg:storeysAboveGround
test_validation!(
    non_negative_integer_zero_valid,
    r#"<?xml version="1.0"?>
<value>0</value>"#,
    XSD_NON_NEGATIVE_INTEGER,
    true
);
test_validation!(
    non_negative_integer_positive_valid,
    r#"<?xml version="1.0"?>
<value>100</value>"#,
    XSD_NON_NEGATIVE_INTEGER,
    true
);
test_validation!(
    non_negative_integer_negative_invalid,
    r#"<?xml version="1.0"?>
<value>-1</value>"#,
    XSD_NON_NEGATIVE_INTEGER,
    false
);

// xs:positiveInteger
test_validation!(
    positive_integer_one_valid,
    r#"<?xml version="1.0"?>
<value>1</value>"#,
    XSD_POSITIVE_INTEGER,
    true
);
test_validation!(
    positive_integer_zero_invalid,
    r#"<?xml version="1.0"?>
<value>0</value>"#,
    XSD_POSITIVE_INTEGER,
    false
);
test_validation!(
    positive_integer_negative_invalid,
    r#"<?xml version="1.0"?>
<value>-1</value>"#,
    XSD_POSITIVE_INTEGER,
    false
);

// ===========================================================================
// xs:decimal
// ===========================================================================
// XSD 1.0 §3.2.3: optional sign followed by either
//   - one or more digits, optionally followed by `.` and zero or more digits
//     (`42`, `42.`, `42.5`)
//   - or `.` followed by one or more digits (`.5`)
// No exponent (that's `xs:double` / `xs:float`).

const XSD_DECIMAL: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="value" type="xs:decimal"/>
</xs:schema>"#;

test_validation!(
    decimal_integer_valid,
    r#"<?xml version="1.0"?>
<value>0</value>"#,
    XSD_DECIMAL,
    true
);
test_validation!(
    decimal_fraction_valid,
    r#"<?xml version="1.0"?>
<value>1.5</value>"#,
    XSD_DECIMAL,
    true
);
test_validation!(
    decimal_negative_valid,
    r#"<?xml version="1.0"?>
<value>-1.5</value>"#,
    XSD_DECIMAL,
    true
);
test_validation!(
    decimal_leading_dot_valid,
    r#"<?xml version="1.0"?>
<value>.5</value>"#,
    XSD_DECIMAL,
    true
);
test_validation!(
    decimal_trailing_dot_valid,
    r#"<?xml version="1.0"?>
<value>1.</value>"#,
    XSD_DECIMAL,
    true
);

test_validation!(
    decimal_exponent_invalid,
    r#"<?xml version="1.0"?>
<value>1e2</value>"#,
    XSD_DECIMAL,
    false
);
test_validation!(
    decimal_alpha_invalid,
    r#"<?xml version="1.0"?>
<value>abc</value>"#,
    XSD_DECIMAL,
    false
);
test_validation!(
    decimal_two_dots_invalid,
    r#"<?xml version="1.0"?>
<value>1.5.6</value>"#,
    XSD_DECIMAL,
    false
);
test_validation!(
    decimal_empty_invalid,
    r#"<?xml version="1.0"?>
<value></value>"#,
    XSD_DECIMAL,
    false
);

// ===========================================================================
// xs:double  (and xs:float — same lexical space)
// ===========================================================================
// XSD 1.0 §3.2.5: a decimal numeral followed by an optional `e`/`E` exponent,
// or one of the special literals `INF`, `-INF`, `NaN` (case-sensitive).

const XSD_DOUBLE: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="value" type="xs:double"/>
</xs:schema>"#;

const XSD_FLOAT: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="value" type="xs:float"/>
</xs:schema>"#;

test_validation!(
    double_integer_valid,
    r#"<?xml version="1.0"?>
<value>0</value>"#,
    XSD_DOUBLE,
    true
);
test_validation!(
    double_fraction_valid,
    r#"<?xml version="1.0"?>
<value>1.5</value>"#,
    XSD_DOUBLE,
    true
);
test_validation!(
    double_negative_exponent_valid,
    r#"<?xml version="1.0"?>
<value>-1.5e-3</value>"#,
    XSD_DOUBLE,
    true
);
test_validation!(
    double_positive_exponent_valid,
    r#"<?xml version="1.0"?>
<value>1.2E10</value>"#,
    XSD_DOUBLE,
    true
);
test_validation!(
    double_inf_valid,
    r#"<?xml version="1.0"?>
<value>INF</value>"#,
    XSD_DOUBLE,
    true
);
test_validation!(
    double_negative_inf_valid,
    r#"<?xml version="1.0"?>
<value>-INF</value>"#,
    XSD_DOUBLE,
    true
);
test_validation!(
    double_nan_valid,
    r#"<?xml version="1.0"?>
<value>NaN</value>"#,
    XSD_DOUBLE,
    true
);

test_validation!(
    double_alpha_invalid,
    r#"<?xml version="1.0"?>
<value>abc</value>"#,
    XSD_DOUBLE,
    false
);
test_validation!(
    double_two_dots_invalid,
    r#"<?xml version="1.0"?>
<value>1.5.6</value>"#,
    XSD_DOUBLE,
    false
);
test_validation!(
    double_lowercase_inf_invalid,
    r#"<?xml version="1.0"?>
<value>inf</value>"#,
    XSD_DOUBLE,
    false
);
test_validation!(
    double_empty_invalid,
    r#"<?xml version="1.0"?>
<value></value>"#,
    XSD_DOUBLE,
    false
);

// xs:float — shares the lexical space; sanity check that base resolution
// dispatches to the same validator.
test_validation!(
    float_fraction_valid,
    r#"<?xml version="1.0"?>
<value>3.14</value>"#,
    XSD_FLOAT,
    true
);
test_validation!(
    float_alpha_invalid,
    r#"<?xml version="1.0"?>
<value>abc</value>"#,
    XSD_FLOAT,
    false
);

// ===========================================================================
// xs:date  (PLATEAU: `core:creationDate`, `core:terminationDate`)
// ===========================================================================
// XSD 1.0 §3.2.9: `-?YYYY-MM-DD(Z|[+-]HH:MM)?`. The year is at least 4 digits.
// Month must be 01..12, day must be in the valid range for the month.

const XSD_DATE: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="value" type="xs:date"/>
</xs:schema>"#;

test_validation!(
    date_simple_valid,
    r#"<?xml version="1.0"?>
<value>2026-05-28</value>"#,
    XSD_DATE,
    true
);
test_validation!(
    date_utc_tz_valid,
    r#"<?xml version="1.0"?>
<value>2026-05-28Z</value>"#,
    XSD_DATE,
    true
);
test_validation!(
    date_jst_tz_valid,
    r#"<?xml version="1.0"?>
<value>2026-05-28+09:00</value>"#,
    XSD_DATE,
    true
);

test_validation!(
    date_bad_month_invalid,
    r#"<?xml version="1.0"?>
<value>2026-13-01</value>"#,
    XSD_DATE,
    false
);
test_validation!(
    date_bad_day_invalid,
    r#"<?xml version="1.0"?>
<value>2026-02-30</value>"#,
    XSD_DATE,
    false
);
test_validation!(
    date_short_year_invalid,
    r#"<?xml version="1.0"?>
<value>26-05-28</value>"#,
    XSD_DATE,
    false
);
test_validation!(
    date_slash_separator_invalid,
    r#"<?xml version="1.0"?>
<value>2026/05/28</value>"#,
    XSD_DATE,
    false
);
test_validation!(
    date_alpha_invalid,
    r#"<?xml version="1.0"?>
<value>abc</value>"#,
    XSD_DATE,
    false
);

// ===========================================================================
// xs:gYear  (PLATEAU: `bldg:yearOfConstruction`, `bldg:yearOfDemolition`)
// ===========================================================================
// XSD 1.0 §3.2.11: `-?YYYY(Z|[+-]HH:MM)?`. The year is at least 4 digits.

const XSD_GYEAR: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="value" type="xs:gYear"/>
</xs:schema>"#;

test_validation!(
    gyear_simple_valid,
    r#"<?xml version="1.0"?>
<value>2026</value>"#,
    XSD_GYEAR,
    true
);
test_validation!(
    gyear_utc_tz_valid,
    r#"<?xml version="1.0"?>
<value>2026Z</value>"#,
    XSD_GYEAR,
    true
);
test_validation!(
    gyear_jst_tz_valid,
    r#"<?xml version="1.0"?>
<value>2026+09:00</value>"#,
    XSD_GYEAR,
    true
);

test_validation!(
    gyear_short_invalid,
    r#"<?xml version="1.0"?>
<value>26</value>"#,
    XSD_GYEAR,
    false
);
test_validation!(
    gyear_with_month_invalid,
    r#"<?xml version="1.0"?>
<value>2026-05</value>"#,
    XSD_GYEAR,
    false
);
test_validation!(
    gyear_alpha_invalid,
    r#"<?xml version="1.0"?>
<value>abc</value>"#,
    XSD_GYEAR,
    false
);

// ===========================================================================
// xs:dateTime
// ===========================================================================
// XSD 1.0 §3.2.7: `xs:date` form (without TZ) + `T` + `HH:MM:SS(.fff)?` +
// optional `(Z|[+-]HH:MM)` timezone. Seconds are required; the separator is
// uppercase `T` (no space).

const XSD_DATETIME: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="value" type="xs:dateTime"/>
</xs:schema>"#;

test_validation!(
    datetime_simple_valid,
    r#"<?xml version="1.0"?>
<value>2026-05-28T10:30:00</value>"#,
    XSD_DATETIME,
    true
);
test_validation!(
    datetime_fractional_seconds_valid,
    r#"<?xml version="1.0"?>
<value>2026-05-28T10:30:00.123</value>"#,
    XSD_DATETIME,
    true
);
test_validation!(
    datetime_utc_tz_valid,
    r#"<?xml version="1.0"?>
<value>2026-05-28T10:30:00Z</value>"#,
    XSD_DATETIME,
    true
);
test_validation!(
    datetime_jst_tz_valid,
    r#"<?xml version="1.0"?>
<value>2026-05-28T10:30:00+09:00</value>"#,
    XSD_DATETIME,
    true
);

test_validation!(
    datetime_space_separator_invalid,
    r#"<?xml version="1.0"?>
<value>2026-05-28 10:30:00</value>"#,
    XSD_DATETIME,
    false
);
test_validation!(
    datetime_missing_seconds_invalid,
    r#"<?xml version="1.0"?>
<value>2026-05-28T10:30</value>"#,
    XSD_DATETIME,
    false
);
test_validation!(
    datetime_bad_month_invalid,
    r#"<?xml version="1.0"?>
<value>2026-13-01T10:30:00</value>"#,
    XSD_DATETIME,
    false
);
