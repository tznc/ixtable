# Microsoft Access file formats

This is the specification the Access importer (`src-tauri/src/access/`) is
written against. Microsoft does not publish these formats. Everything here was
worked out from the 30 featured Access templates, the Jackcess and mdbtools
test databases, files written with Jackcess 4.0.8, and byte-level comparison of
the importer's output against Jackcess. Each section names the module that
implements it. Where a field's meaning is unknown, the section says so.

The importer reads three kinds of file:

| Extension | Container | Engine | Section |
|---|---|---|---|
| `.accdt` | OPC (zip) package of XML and text parts | none, it is a description of a database | §2, §3 |
| `.mdb` | Jet 3 (Access 97) or Jet 4 (Access 2000 to 2003) page file | Jet | §4 |
| `.accdb` | ACE (Access 2007 and later) page file | ACE, a Jet 4 descendant | §4 |

`access::open` tells them apart by the first bytes: `PK` is a template package,
anything else is read as a page file and must carry a Jet or ACE signature.

## 1. Object model

All three formats describe the same objects. The common model is in
`access/model.rs`.

- **Tables** with typed columns, indexes, field and table properties, and rows.
- **Relationships** between tables, with referential-integrity flags.
- **Queries**: Access SQL text or a design-grid definition, plus parameters.
- **Forms, reports, macros**: design objects. Templates store them as
  SaveAsText (§3). Binary files store them compiled.
- **Modules**: VBA source.
- **Database properties**: `AppTitle`, `StartUpForm`, and the like.

Data types use the Jet type codes throughout:

| Code | Type | Fixed size (bytes) | Value encoding |
|---|---|---|---|
| 0x01 | Yes/No (Boolean) | 0 | the column's null-mask bit is the value (§4.5) |
| 0x02 | Byte | 1 | unsigned |
| 0x03 | Integer | 2 | i16 LE |
| 0x04 | Long Integer | 4 | i32 LE |
| 0x05 | Currency | 8 | i64 LE, ten-thousandths |
| 0x06 | Single | 4 | IEEE 754 f32 LE |
| 0x07 | Double | 8 | IEEE 754 f64 LE |
| 0x08 | Date/Time | 8 | f64 days since 1899-12-30, fraction is the time of day |
| 0x09 | Binary | variable | raw bytes, at most 255 |
| 0x0A | Text | variable | §4.6 |
| 0x0B | OLE Object | variable | long value (§4.7) |
| 0x0C | Memo (Long Text) | variable | long value holding text (§4.6) |
| 0x0F | GUID (Replication ID) | 16 | Windows GUID: u32, u16, u16 LE, then 8 bytes |
| 0x10 | Decimal | 17 | sign byte (0x80 negative), four u32 LE words, most significant word first, scaled by the column's scale |
| 0x11 | Complex (Jet 4 placeholder) | variable | treated as binary |
| 0x12 | Complex (ACE) | 4 | i32 key into the column's flat table (§4.11) |
| 0x13 | Large Number (BigInt) | 8 | i64 LE |
| 0x14 | Date/Time Extended | 42 | ASCII text `YYYY-MM-DD HH:MM:SS.fffffff` |

AutoNumber is a Long Integer (or a GUID) with an auto-number flag. Hyperlink is
a Memo with a flag. Its value is `display#address#subaddress#screentip`.
Attachment and multi-value fields are complex columns (§4.11).

## 2. Template packages (`.accdt`)

Module: `access/accdt.rs`.

A template is an Open Packaging Conventions zip with `[Content_Types].xml`,
`_rels/.rels`, and `docProps/core.xml` at the root. Microsoft serves the
featured templates as
`https://omextemplates.content.office.net/support/templates/en-us/<id>.accdt`,
where `<id>` is the template's `tf` number.

```text
template/
  database/
    databaseProperties.xml       database properties (UTF-16)
    relationships.xml            rows of MSysRelationships
    navpane.xml                  navigation pane groups (not imported)
    vbaReferences.xml            VBA project references (not imported)
    objects/
      table<Stem>.xsd            one per table: schema with od: annotations
      query<Stem>.txt            SaveAsText query definition (§3.3)
      form<Stem>.txt             SaveAsText form (§3)
      report<Stem>.txt           SaveAsText report (§3)
      macro<Stem>.txt            SaveAsText macro (§3.4)
      module<Stem>.txt           VBA source
      sampleData/table<Stem>.xml rows for the table
      properties/<part>_Metadata.xml   real object name and name map
      properties/<part>_Properties.axl object properties (not imported)
    resources/ResN, ResN-name.txt      shared images and themes
```

### 2.1 Object names

A part's stem drops characters that file names cannot hold, so it is not the
object's name. `properties/<part>_Metadata.xml` holds the real one:

```xml
<AccessObject xmlns="http://schemas.microsoft.com/office/access/2005/04/template/object-metadata">
  <Type>Form</Type><Name>Asset Details</Name><NameMap>...</NameMap>
</AccessObject>
```

When no metadata part exists, the importer uses the stem.

### 2.2 Table schema parts

Each `table<Stem>.xsd` is an XML Schema whose `dataroot` element lists one
element per table (normally one). The table element's `xsd:annotation/xsd:appinfo`
holds `od:index` and `od:tableProperty` elements. Each column is an
`xsd:element` in the table's `xsd:sequence`:

```xml
<xsd:element name="ID" minOccurs="1" od:jetType="autonumber" od:sqlSType="int"
             od:autoUnique="yes" od:nonNullable="yes" type="xsd:int">
  <xsd:annotation><xsd:appinfo>
    <od:fieldProperty name="Required" type="1" value="0"/>
    <od:fieldProperty name="ColumnWidth" type="3" value="585"/>
  </xsd:appinfo></xsd:annotation>
</xsd:element>
```

- Element and index-key names escape characters as `_xHHHH_` (UTF-16 code unit
  in hex): `First_x0020_Name` is `First Name`. `xml::unescape_name` reverses
  it.
- `od:jetType` is the column type: `yesno`, `byte`, `integer`, `longinteger`,
  `autonumber`, `currency`, `single`, `double`, `datetime`,
  `datetimeextended`, `binary`, `oleobject`, `memo`, `hyperlink`, `text`,
  `replicationid` or `guid`, `bigint` or `largenumber`, `decimal`, `complex`.
- `od:autoUnique="yes"` marks an AutoNumber. `od:nonNullable="yes"` or a
  `Required` property of 1 makes the column required.
- Text length is `xsd:maxLength` inside `xsd:simpleType/xsd:restriction`.
  Decimal precision and scale are `xsd:totalDigits` and `xsd:fractionDigits`.
- A calculated column has `od:expression="..."` (the Access expression) and a
  `ResultType` field property with the Jet type code of its result.
- A complex column has `od:jetType="complex"`, `maxOccurs="unbounded"`, and
  `od:jetComplexType` naming its type: `MSysComplexType_Attachment`, or
  `MSysComplexType_Text`, `_Long`, `_Short`, `_UnsignedByte`, `_IEEESingle`,
  `_IEEEDouble`, `_GUID`, `_Decimal` for multi-value fields.
- `od:fieldProperty` and `od:tableProperty` carry DAO properties. `type` is the
  DAO type number (1 Boolean, 2 Byte, 3 Integer, 4 Long, 10 Text, 11 binary as
  base64, 12 Memo). The importer keeps them as strings. The ones it uses:
  `Caption`, `Description`, `DefaultValue`, `ValidationRule`,
  `ValidationText`, `Format`, `InputMask`, `DisplayControl`, `RowSource`,
  `RowSourceType`, `BoundColumn`, `ColumnCount`, `ColumnWidths`,
  `LimitToList`, `ShowDatePicker`, `TextFormat` (1 is rich text), `ResultType`,
  `Expression`, and the table's own `ValidationRule` and `ValidationText`.
- `od:index` has `index-name`, `index-key` (space-separated escaped column
  names, each optionally followed by ` desc`), `primary`, `unique`, and
  `order`.

Some templates repeat a table across parts, for example a Furigana copy. The
importer keeps the first and reports the duplicate.

### 2.3 Sample data parts

`sampleData/table<Stem>.xml` repeats the schema inline in an `xsd:schema`
element, then lists rows as `<dataroot>` children named after the table, with
one child element per non-null column:

```xml
<Assets>
  <ID>1</ID>
  <Item>Laptop</Item>
  <Acquired_x0020_Date>2017-01-15T00:00:00</Acquired_x0020_Date>
  <Attachments>
    <FileData>base64...</FileData><FileName>photo.png</FileName><FileType>png</FileType>
  </Attachments>
</Assets>
```

- Yes/No is `1`/`-1` (true) or `0`. Dates are ISO 8601 without a zone.
  Currency and decimals are plain decimal text. Binary and OLE values are
  base64.
- A multi-value field repeats its element once per value, each holding
  `<Value>`. An attachment field repeats its element once per file.
- `FileData` is base64 of the attachment content: a header, then the file.
  The header is u32 header length (including itself), u32 constant 1, u32
  extension length in characters including the terminator, then the extension
  as null-terminated UTF-16LE. `blob::split_attachment` reads it. Binary files
  wrap the same content once more (§4.11).

### 2.4 Relationships and database properties

`relationships.xml` is a `dataroot` of `MSysRelationships` rows with the same
columns as the system table (§4.8): `szRelationship`, `szObject`, `szColumn`,
`szReferencedObject`, `szReferencedColumn`, `icolumn` (position within a
multi-column relationship), `ccolumn` (column count), and `grbit` (flags).
`merge_relationship_rows` groups rows by name and orders columns by `icolumn`.

`databaseProperties.xml` is UTF-16 XML listing `<Property Name="..."
Type="..." Value="..."/>` elements. `AppTitle` names the imported document and
`StartUpForm` names the start page.

## 3. SaveAsText

Module: `access/text_format.rs`. This is the text that Access writes with the
undocumented `Application.SaveAsText` method and reads back with
`LoadFromText`. Templates store forms, reports, macros, and queries this way.

### 3.1 Encoding

Forms and reports are usually UTF-16LE with a BOM (`FF FE`). Queries and
macros may be UTF-8 (with or without BOM) or Windows-1252. `text_format::decode`
checks for a UTF-16 BOM, then tries UTF-8, then falls back to Windows-1252.

### 3.2 Grammar

The format is line based. Leading whitespace is indentation only.

```text
file       := (version-line | prop | block)* [ "CodeBehindForm" vba ]
version    := "Version =" number | "VersionRequired =" number | "Checksum =" number
block      := "Begin" [kind] NL (prop | block)* "End"
prop       := key " =" value
            | dao-type " " quoted-key " =" value        (typed property)
            | key " = Begin" NL (hex-line* | (prop | block)*) "End"
value      := quoted+ | token
quoted     := '"' chars '"'                               (continues on next line
                                                           when that line is only
                                                           another quoted string)
token      := -1 | 0 | 65.0 | NotDefault | 0x00000000 ...
hex-line   := "0x" hexdigits [","]
```

- Inside a quoted string, `\"` is a quote, `\\` a backslash, and `\NNN` an
  octal byte (`\015\012` is CR LF). Concatenate continuation lines with no
  separator.
- `Key = Begin` followed by `0x...` lines is binary data (for example
  `PrtMip`, `PrtDevMode`, `PictureData`). Followed by properties, it is a
  nested property block. Embedded macros use this:
  `OnClickEmMacro = Begin ... End`.
- Keys repeat. A macro block lists `Argument` several times, and order
  matters.
- Everything after a line holding only `CodeBehindForm` is the form's VBA
  module.

A form or report file has one top `Begin Form` or `Begin Report` block. It
holds form properties, then optional default blocks per control kind (`Begin
Label`, `Begin TextBox` with no name, setting defaults), then `Begin Section`
blocks. A section's `Begin` block lists controls. Each control is a block named
after its kind (`Label`, `TextBox`, `ComboBox`, `ListBox`, `CheckBox`,
`OptionGroup`, `OptionButton`, `ToggleButton`, `CommandButton`, `Subform`,
`Image`, `Line`, `Rectangle`, `Tab` with `Page` children, `Attachment`,
`EmptyCell`, `WebBrowser`, `NavigationControl`, `NavigationButton`). A label
attached to a control is nested inside the control's block.

Section order in a form: `FormHeader`, `PageHeader`, `Detail` (unnamed in older
files), `PageFooter`, `FormFooter`. Reports add group headers and footers
between page header and detail, in `BreakLevel` order, with `GroupHeader =-1`
and `GroupFooter =-1` on the break level.

Geometry is in twips (1/1440 inch): `Left`, `Top`, `Width`, `Height`, and the
section's `Height`. `LayoutCachedLeft` and friends repeat them for layout-view
controls.

Property values the importer relies on:

| Property | Meaning |
|---|---|
| `RecordSource` | table, query, or SQL text |
| `DefaultView` | 0 single form, 1 continuous, 2 datasheet, 3 pivot table, 4 pivot chart, 5 split form |
| `AllowAdditions`, `AllowEdits`, `AllowDeletions`, `DataEntry` | -1 true, 0 false |
| `ControlSource` | field name, or `=expression` for a calculated control |
| `RowSourceType`, `RowSource`, `BoundColumn`, `ColumnCount`, `ColumnWidths` | combo and list box lookups |
| `SourceObject` | `Form.Name`, `Report.Name`, `Table.Name`, `Query.Name` |
| `LinkMasterFields`, `LinkChildFields` | subform link, `;`-separated |
| `Visible`, `Locked`, `Enabled`, `TabStop`, `Format`, `DefaultValue`, `ValidationRule`, `ValidationText` | control behavior |
| `OnClick`, `OnLoad`, `OnOpen`, `OnCurrent`, `AfterUpdate`, ... | `[Event Procedure]` (VBA), `[Embedded Macro]`, or a macro name |
| `Picture`, `PictureData` | image name and bytes (§4.11 for the shared image store) |
| `GroupOn`, `GroupInterval`, `SortOrder`, `ControlSource` in `BreakLevel` | report sorting and grouping |
| `PrtMip` | report margins: u32 left, top, right, bottom in twips at offset 0 |
| `PrtDevModeW`, `PrtDevMode` | Windows `DEVMODEW` (orientation u16 at 76, paper size at 78) or ANSI `DEVMODEA` (44 and 46). Orientation 1 is portrait, 2 landscape |

### 3.3 Query definitions

Module: `access/query_def.rs`. A query file is either plain SQL or a
design-grid definition:

```text
Operation =1
Option =0
Where ="(((Orders.Paid)=False))"
Begin InputTables
    Name ="Orders"
    Name ="Customers"
    Alias ="c"
End
Begin OutputColumns
    Expression ="Orders.ID"
    Alias ="Total"
    Expression ="Sum([Amount])"
End
Begin Joins
    LeftTable ="Customers"
    RightTable ="Orders"
    Expression ="Customers.ID = Orders.Customer"
    Flag =1
End
Begin OrderBy
    Expression ="Orders.ID"
    Flag =1
End
Begin Groups
    Expression ="Orders.ID"
    GroupLevel =0
End
dbMemo "SQL" ="SELECT ..."
dbText "Description" ="..."
```

`Operation` is the query type:

| Value | Kind |
|---|---|
| 1 | select |
| 2 | make-table (`SELECT ... INTO`) |
| 3 | append (`INSERT INTO`) |
| 4 | update |
| 5 | delete |
| 6 | crosstab (`TRANSFORM ... PIVOT`) |
| 7 | data definition |
| 8 | pass-through |
| 9 | union |

`Option` is a bit set: 1 output all fields (`SELECT *`), 2 `DISTINCT`, 4
`WITH OWNERACCESS OPTION`, 8 `DISTINCTROW`, 16 top values (`RowCount` holds the
number), 32 top percent. Join `Flag` is 1 inner, 2 left, 3 right. `OrderBy`
`Flag` 1 is descending. A `Begin Parameters` block lists `Name` and `Flag`
pairs, and `Flag` is the DAO type number. An `OutputColumns` entry in an append or
update query names the target column in `Name`.

When a `dbMemo "SQL"` property exists, it is the authoritative SQL and the
importer uses it. Otherwise `query_def::to_access_sql` rebuilds the SQL from the
grid. Joins nest left to right. A join condition whose tables are already inside
an earlier join is added to the innermost join that contains both tables.

### 3.4 Macros and AXL

Module: `access/convert/macros.rs`. Since Access 2010 a macro stores two forms
of itself:

1. **AXL**, the macro XML, split into chunks across `Comment ="_AXL:..."`
   properties. Concatenate the chunks after the `_AXL:` prefix. The root is
   `<UserInterfaceMacro>` (or `<DataMacros>` for table events) holding
   `<Statements>` and named `<Sub Name="...">` submacros. Statements are
   `<Action Name="OpenForm"><Argument Name="FormName">...</Argument></Action>`,
   `<ConditionalBlock><If><Condition>...</Condition><Statements/></If>
   <ElseIf/>...<Else/></ConditionalBlock>`, `<Comment>`, and `<Group>`.
2. **Legacy rows**, one `Begin` block per action, with `Action`, `Condition`
   (where `...` continues the previous condition), and positional `Argument`
   values. Argument order is fixed per action: OpenForm is `FormName`, `View`,
   `FilterName`, `WhereCondition`, `DataMode`, `WindowMode`.

A standalone macro file has `Version`, `ColumnsShown`, and the action blocks.
An embedded macro is the same content inside a `= Begin` property of a control
or form. Submacros in a standalone macro are called as `Macro.Sub`. A macro
named `AutoExec` runs when the database opens.

Enumerations used in arguments: OpenForm `View` 0 form, 1 design, 2 print
preview, 3 datasheet. `DataMode` 0 add, 1 edit, 2 read only. OpenReport `View`
0 print, 1 design, 2 print preview, 5 report view.

## 4. Jet and ACE page files

Module: `access/jet/`. A database file is an array of fixed-size pages: 2048
bytes for Jet 3, 4096 for Jet 4 and ACE. All integers are little-endian. A
"row pointer" is a u32 with the page number in the upper 24 bits and the row
number in the low 8.

Page types (byte 0 of the page):

| Byte | Page |
|---|---|
| 0x00 | database header (page 0 only) |
| 0x01 | data page |
| 0x02 | table definition (TDEF) |
| 0x03 | intermediate index page |
| 0x04 | leaf index page |
| 0x05 | usage map page |

### 4.1 Header page

Module: `jet/page.rs` (`parse_header`).

| Offset | Size | Field |
|---|---|---|
| 0x00 | 1 | 0x00 |
| 0x04 | 15 | `Standard Jet DB` or `Standard ACE DB` (`MSISAM Database` is Microsoft Money) |
| 0x14 | 1 | version: 0 Jet 3, 1 Jet 4, 2 ACE 12 (2007), 3 ACE 14 (2010), 5 ACE 16 (2016), 6 ACE 16 with Large Number support (2019) |
| 0x18 | 126 or 128 | masked header bytes (below) |

Bytes from 0x18 are XORed with a fixed 128-byte mask, `page::HEADER_MASK`. It
is the RC4 keystream of the key `0x6B39DAC7` (4 bytes, little-endian), and
starts `B5 6F 03 62 61 08 C2 55`. Jackcess and mdbtools store the same table.
Jet 3 masks 126 bytes, Jet 4 and ACE mask 128. After unmasking:

| Offset | Size | Field |
|---|---|---|
| 0x3C | 2 | Windows code page of Jet 3 text (0 means the system default. Read as 1252) |
| 0x3E | 4 | page encoding key, 0 when pages are not encrypted |
| 0x42 | 20 (Jet 3) or 40 (Jet 4/ACE, UTF-16) | database password, all zero when there is none (below) |
| 0x72 | 8 | creation date (f64). Jet 4 and ACE mask the password with it |

### 4.2 Passwords and encryption

The importer reads only unprotected files. `page::protection_error` refuses
the rest, and the user removes the protection in Access first.

- **Jet 3/4 database password.** The password bytes at 0x42 are non-zero. In
  Jet 4 and ACE they are also XORed with the creation date: take the f64 at
  0x72, truncate it to an i32, and XOR its 4 little-endian bytes over the
  password, repeating. Only Access checks this password. The pages themselves
  are readable.
- **Jet 3/4 encryption** ("Encrypt Database"). The encoding key at 0x3E is
  non-zero, and every page after page 0 is RC4-encrypted with the key
  `page_number XOR encoding_key` (a little-endian u32).
- **ACE password.** Setting a password in Access 2007 or later encrypts the
  file with Office encryption. Access 2007 and the "legacy encryption" option
  use standard encryption (RC4 or AES-128 with SHA-1). Later versions default
  to agile encryption (AES-256 with SHA-512). The encoding key is non-zero and
  the header holds the encryption parameters.

### 4.3 Data pages and usage maps

A data page:

| Jet 3 | Jet 4/ACE | Field |
|---|---|---|
| 0 | 0 | 0x01 |
| 2 | 2 | free space in bytes |
| 4 | 4 | page number of the owning TDEF |
| 8 | 12 | u16 row count |
| 10 | 14 | u16 row offsets, one per row |

Rows are packed from the end of the page toward the front. Row `n` spans from
its offset to the offset of row `n-1` (row 0 runs to the end of the page). Each
offset's low 13 bits are the position. Bit 0x8000 marks a deleted row, bit
0x4000 an overflow row: the row's first 4 bytes are a row pointer to where the
row now lives. Follow it and read that row instead.

A **usage map** lists the pages a table owns. The TDEF holds a row pointer to
it (§4.4). The map row's first byte is its type:

- Type 0 (inline): u32 first page number, then a bitmap. Bit `i` set means page
  `first + i` belongs to the table.
- Type 1 (reference): an array of u32 page numbers of usage map pages (type
  0x05). Each map page has a 4-byte header, then a bitmap covering
  `(page_size - 4) * 8` pages. Map page `i` covers pages from
  `i * (page_size - 4) * 8`. A zero entry is a gap.

To read a table, take the pages from its owned-pages map, keep the data pages
whose TDEF pointer matches, and read every row that is not deleted (following
overflow pointers).

### 4.4 Table definitions (TDEF)

Module: `jet/tdef.rs`. A TDEF starts with page type 0x02 and a u32 at offset 4
pointing to the next TDEF page (0 for the last). Concatenate the pages, keeping
all of the first and skipping the 8-byte header of the rest.

Header fields:

| Jet 3 | Jet 4/ACE | Size | Field |
|---|---|---|---|
| 8 | 8 | 4 | definition length |
| 12 | 16 | 4 | row count |
| 16 | 20 | 4 | next AutoNumber value |
| 20 | 40 | 1 | table type: 0x53 system, 0x4E user |
| 21 | 41 | 2 | next column number |
| 23 | 43 | 2 | variable-length column count |
| 25 | 45 | 2 | column count |
| 27 | 47 | 4 | logical index count |
| 31 | 51 | 4 | physical ("real") index count |
| 35 | 55 | 4 | row pointer to the owned-pages usage map |
| 39 | 59 | 4 | row pointer to the free-space usage map |
| 43 | 63 | | start of the real-index count block |

After the header comes one entry per real index (8 bytes in Jet 3, 12 in Jet
4, holding its row count), then the column blocks.

Column block (18 bytes in Jet 3, 25 in Jet 4/ACE):

| Jet 3 | Jet 4/ACE | Size | Field |
|---|---|---|---|
| 0 | 0 | 1 | type code (§1) |
| 1 | 5 | 2 | column number (position in the null mask) |
| 3 | 7 | 2 | index into the variable-offset table |
| 5 | 9 | 2 | column index in creation order |
| 7 to 12 | 11 to 14 | | type-specific. Text: collation and code page. Decimal: precision at byte 11, scale at byte 12 (both versions). ACE complex: u32 complex id at 11 |
| 13 | 15 | 1 | flags: 0x01 fixed length, 0x02 can be null, 0x04 AutoNumber, 0x40 GUID AutoNumber (replication), 0x80 hyperlink |
| | 16 | 1 | extended flags: 0xC0 together mark a calculated column (ACE 14+) |
| 14 | 21 | 2 | offset within the fixed area |
| 16 | 23 | 2 | length in bytes |

Column names follow the column blocks in the same order: Jet 3 a 1-byte
length then code-page bytes, Jet 4/ACE a u16 byte length then UTF-16LE.
Columns are listed in creation order. Sort them by column number for display
order.

Real-index blocks follow the names (39 bytes in Jet 3, 52 in Jet 4/ACE). Jet 4
starts each with 4 unknown bytes. Then come 10 slots of u16 column number
(0xFFFF unused) plus one order byte (0x01 ascending), a u32 used-pages pointer,
a u32 first data page, and the flags byte: 0x01 unique, 0x02 ignore nulls,
0x08 required. The flags byte is at offset 38 in Jet 3 (the last byte) and 46
in Jet 4.

Logical-index blocks follow (20 bytes in Jet 3, 28 in Jet 4/ACE, Jet 4 again
with 4 leading unknown bytes). After those bytes: u32 logical number, u32 real
index number, then relationship fields, and at relative offset 19 the index
type: 1 primary key, 2 foreign key (an index Access creates for a
relationship). Logical-index names follow in the same name encoding.

### 4.5 Rows

Module: `jet/row.rs` (`split`). A row is:

```text
Jet 4/ACE:  u16 column count | fixed area | variable data | var offsets (reversed) | u16 var count | null mask
Jet 3:      u8 column count  | fixed area | variable data | jump table | var offsets (reversed) | u8 var count | null mask
```

- The **null mask** is the last `ceil(column_count / 8)` bytes. Bit `n` (LSB
  first) is set when column number `n` has a value. A Yes/No column has no
  data. Its mask bit is the value.
- A **fixed column** lives at `1 + fixed_offset` (Jet 3) or `2 + fixed_offset`
  (Jet 4) with the size from §1. The column's `length` covers calculated
  columns, whose size differs.
- **Variable columns** are found through the offset table. In Jet 4 the u16
  variable count sits just before the null mask, and the offsets are u16s
  stored backwards before it: offset `i` is at `count_pos - 2 * (i + 1)`. There
  are `count + 1` offsets. Column `var_index` spans offset `var_index` to
  offset `var_index + 1`.
- Jet 3 offsets are single bytes, so rows longer than 255 bytes need a **jump
  table**. It sits between the var count and the offsets and has
  `(row_length - 1) / 256` entries (one fewer if the offset table itself does
  not reach the last 256-byte boundary). Each entry is the index of the first
  variable column that starts past the next 256-byte boundary. An offset's
  real value is its byte plus 256 times the number of jump entries at or below
  its column index.

### 4.6 Text

Module: `jet/text.rs`.

- **Jet 3** text is in the database code page (§4.1).
- **Jet 4 and ACE** text is UTF-16LE, except **compressed unicode**: when a
  value starts with `FF FE`, the rest alternates between runs of one-byte
  characters (Latin-1) and runs of UTF-16LE code units. The first run is
  one-byte. A 0x00 byte switches mode and is not part of the text. Access
  compresses only text columns that have "Unicode Compression" on.
- Object names in the TDEF and in property maps are never compressed.

### 4.7 Long values (Memo, OLE)

`row::long_value`. A Memo or OLE column holds a 12-byte header in the row:

| Offset | Size | Field |
|---|---|---|
| 0 | 4 | length in the low 30 bits. The top two bits are the storage kind |
| 4 | 4 | row pointer to the first LVAL row (kinds 1 and 0) |
| 8 | 4 | unknown |

| Top bits | Storage |
|---|---|
| `10` (byte 3 is 0x80) | inline: the data follows the header in the row |
| `01` (0x40) | one LVAL row holds all the data |
| `00` | a chain of LVAL rows. Each starts with a u32 row pointer to the next (0 ends it). The rest is data |

LVAL rows live on data pages owned by the table (they appear in its usage map
but carry no row data). Truncate the result to the header's length.

An OLE Object field written by Access wraps the file in an OLE header that
starts with `15 1C`: a package or bitmap header followed by the original file.
`blob::ole_payload` looks for a known file signature (PNG, JPEG, GIF, PDF, BMP,
zip) in the first 4 KB and returns the bytes from there.

Dates are f64 days since 1899-12-30. Negative values count back with a
positive time fraction. Currency is an i64 count of ten-thousandths.

**Calculated columns** (ACE 14+, extended flags 0xC0) store their last value in
a 20-byte wrapper: bytes 16 to 19 are the u32 length, data starts at byte 20.
The stored type is the column's `ResultType` property, not the TDEF type code.
A calculated Decimal is an OLE `DECIMAL`: u16 14 (`VT_DECIMAL`), byte scale,
byte sign (0x80 negative), u32 high 32 bits, u64 low 64 bits. A calculated
Yes/No is one byte. A calculated Memo is a long value whose data holds the
wrapper.

### 4.8 System catalog

Module: `jet/catalog.rs`. The TDEF of `MSysObjects` is always page 2. Its
columns the importer reads:

| Column | Meaning |
|---|---|
| `Id` | object id. For a local table, it is also the TDEF page number |
| `ParentId` | containing object (`Tables`, `Forms`, `Reports`, ...) |
| `Name` | object name |
| `Type` | i16: 1 local table, 4 ODBC linked table, 5 query, 6 linked table, 8 relationship, -32768 form, -32764 report, -32766 macro, -32761 module, 2 and 3 containers |
| `Flags` | 0x80000000 and 0x00000002 mark system objects. For tables, `Flags & 0x000F0000` is 0x000A0000 for a complex column's flat table and 0x00030000 for a complex type table |
| `LvProp` | property maps (§4.9) |
| `Database`, `Connect`, `ForeignName` | link target of a linked table |
| `LvExtra`, `LvModule` | compiled form, report, and module data (see below) |

Names starting with `MSys`, `f_`, or `~` are system or temporary objects.
`MSysDb` is the database itself. Its `LvProp` holds database properties such
as `AppTitle` and `StartUpForm`.

`MSysRelationships` has one row per column of each relationship:
`szRelationship`, `szObject` (the many side), `szColumn`,
`szReferencedObject` (the one side), `szReferencedColumn`, `icolumn`,
`ccolumn`, and `grbit`:

| `grbit` bit | Meaning |
|---|---|
| 0x00000001 | one-to-one |
| 0x00000002 | integrity not enforced |
| 0x00000100 | cascade updates |
| 0x00001000 | cascade deletes |
| 0x00002000 | cascade to null |
| 0x01000000 | left outer join in the query designer |
| 0x02000000 | right outer join in the query designer |

**Compiled design objects.** A binary file stores forms, reports, and macros
in hidden system tables (`MSysAccessObjects` in Jet 4, `MSysAccessStorage` in
ACE) in an undocumented binary format, and modules as compiled VBA p-code with
compressed source. The importer does not
decode them. It lists the objects in the inventory and generates forms instead
(§6). To keep a binary database's forms and reports, save it as a template in
Access (File, Save As, Template) and import the `.accdt`.

### 4.9 Property maps

Module: `jet/props.rs`. `LvProp` starts with `MR2\0` (Jet 4/ACE) or `KKD\0`
(Jet 3). Then come blocks, each a u32 length (including the 6-byte block
header) and a u16 type:

- **Type 0x80, names**: a list of u16 length plus name (UTF-16LE in Jet 4,
  code page in Jet 3). Later blocks refer to names by index.
- **Any other type, values**: u32 name-block length, then the u16
  length and name of the object the values belong to (empty for the table
  itself, else a column name). Then value entries: u16 entry length, u8 DDL
  flag, u8 DAO data type, u16 name index, u16 data length, data.

Data types follow DAO: 1 Boolean, 2 Byte, 3 Integer, 4 Long, 6 Single, 7
Double, 10 Text, 11 binary, 12 Memo, 15 GUID. Properties of interest are the
same as in §2.2.

### 4.10 Queries

`MSysQueries` holds each query as rows sharing `ObjectId` (the query's
`MSysObjects.Id`), sorted by the binary `Order` column. Each row has
`Attribute`, `Expression`, `Flag`, `LvExtra`, `Name1`, `Name2`:

| Attribute | Meaning | Fields |
|---|---|---|
| 0 | start | |
| 1 | type | `Flag` = Operation (§3.3). `Name1` = target table of append and make-table. `Expression` = SQL of DDL and pass-through queries |
| 2 | parameter | `Name1` name, `Flag` DAO type |
| 3 | options | `Flag` = Option bits (§3.3). `Name1` = TOP value. In union queries, `Flag & 0x02` drops `ALL` |
| 4 | remote database | `Name1` connect string |
| 5 | input table or query | `Name1` source, `Name2` alias. In union queries, `Expression` holds one SELECT and `Name2` is `X7YZ_____1` or `X7YZ_____2` |
| 6 | output column | `Expression`, `Name1` alias (in update queries, the value), `Name2` target column. Crosstab: `Flag` 1 is the column heading (PIVOT), 2 a row heading, 0 the value (TRANSFORM). Append: `Flag & 0x8000` marks a criteria-only column |
| 7 | join | `Name1` left, `Name2` right, `Expression` condition, `Flag` 1 inner, 2 left, 3 right |
| 8 | WHERE | `Expression` |
| 9 | GROUP BY | `Expression`. Crosstab row headings have `Flag & 0x02` |
| 10 | HAVING | `Expression` |
| 11 | ORDER BY | `Expression`, `Name1` = `D` for descending |
| 255 | end | |

`catalog::query_sql` rebuilds Access SQL from these rows with the same builder
as §3.3.

### 4.11 Complex columns and attachments

ACE stores attachment and multi-value fields outside the row. The row holds an
i32 key (type 0x12). `MSysComplexColumns` maps each complex column:

| Column | Meaning |
|---|---|
| `ColumnName` | the column in the user table |
| `ComplexID` | id in the TDEF column block (§4.4) |
| `ConceptualTableID` | `MSysObjects.Id` of the user table |
| `ComplexTypeObjectID` | the type table. Its name says the kind (§2.2), and `MSysComplexTypeVH_*` marks an append-only memo's version history |
| `FlatTableID` | TDEF page of the hidden flat table holding the values |

The flat table (`f_<guid>_<column>`) has one row per value, with a foreign key
column holding the row's key. Multi-value flat tables have a `value` column.
Attachment flat tables have `FileData`, `FileName`, `FileType`,
`FileURL`, `FileTimeStamp`, and `FileFlags`.

Attachment `FileData` is wrapped: u32 flag (0 raw, 1 zlib-deflated with a zlib
header), u32 inflated length, then the content of §2.3 (extension header plus
file). `blob::unwrap_attachment` removes the wrapper.

Version-history tables keep earlier values of an append-only memo. The memo
itself holds the latest text, so the importer drops the history.

Images shared by forms (Access 2010+) live in `MSysResources` (`Name`, `Type`,
`Data` attachment). Templates store them under `template/database/resources/`
as `ResN` (the bytes) and `ResN-name.txt` (UTF-16LE name without BOM).

## 5. Known gaps

- Compiled forms, reports, and VBA in binary files (§4.8).
- Password-protected and encrypted files (§4.2), by design.
- Index B-tree pages (0x03, 0x04) are not needed to read data and are not
  parsed.
- Data macros (table events) are read from AXL but have no ixtable equivalent.
- Linked tables keep no data. The inventory lists them as warnings.
- Unknown bytes are noted in the tables above.

## 6. Conversion to ixtable

Module: `access/convert/` and `access/translate/`. The decision record is
`docs/decisions/access-import.md`. In short:

- **Tables** become SQLite tables with the same names, typed as in
  `schema::declared_type` (Currency `DECIMAL(15,4)`, Date/Time `TIMESTAMP` or
  `DATE` when the format shows only a date, GUID `UUID`, OLE and attachments
  `BLOB`). Required, validation rules, defaults, and unique indexes become
  constraints when the Access expression translates to SQLite. Calculated
  columns become plain columns kept current by triggers.
- **Attachments and multi-value fields** become child tables
  (`<Table> <Column>`) with a foreign key and cascade delete.
- **Relationships** with enforced integrity become foreign keys. Rows that break
  them are reported, not dropped.
- **Queries** are translated from Access SQL to DuckDB SQL
  (`translate/sql.rs`): `[Name]` quoting, `table!field`, `IIf`, `Nz`, `Like`
  with `*` and `?`, `#date#` literals, VBA date and text functions, date
  arithmetic, `TRANSFORM ... PIVOT` to `PIVOT`, union queries, and parameters
  (`[Enter a date]`) as `$enter_a_date` saved-query parameters. Append,
  update, delete and make-table queries become action queries
  (`translate/dml.rs`), as do RunSQL macro actions.
- **Expressions** in forms, reports, and macros are translated to the ixtable
  expression language (`translate/expr.rs`), and `Format` patterns to ixtable
  format patterns (`translate/format.rs`).
- **Forms** keep their layout (twips to grid rows and columns), bound controls,
  lookups, option groups, tabs, images, and subforms (as related lists).
  Datasheet and continuous forms become list forms.
- **Reports** keep their bands, grouping, sorting, totals, page setup, and
  calculated controls.
- **Macros** become actions: OpenForm, OpenReport, MessageBox, SetTempVar,
  RunMacro, GoToRecord, and conditions. Window and focus actions are dropped
  without loss. Others are reported.
- **VBA** is kept as the document asset `Access VBA.txt`.
- **Binary files** get a generated list form and detail form per table.

Every object gets a status (converted, partly converted, not converted) and
notes, shown in the wizard and stored in `settings.accessImport.report`.
