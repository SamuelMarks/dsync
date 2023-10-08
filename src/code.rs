use heck::{ToPascalCase, ToSnakeCase};
use indoc::indoc;
use std::borrow::Cow;

use crate::parser::{ParsedColumnMacro, ParsedTableMacro, FILE_SIGNATURE};
use crate::{GenerationConfig, TableOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StructType {
    /// Variant for the `Read` struct which can be queried and has all properties
    Read,
    /// Variant for a `Update*` struct, which has all properties wrapped in [`Option<>`]
    Update,
    /// Variant for a `Create` struct, which only has all the properties which are not autogenerated
    Create,
}

impl StructType {
    /// Get the prefix for the current [StructType]
    ///
    /// Example: `UpdateTodos`
    pub fn prefix(&self) -> &'static str {
        match self {
            StructType::Read => "",
            StructType::Update => "Update",
            StructType::Create => "Create",
        }
    }

    /// Get the suffix for the current [StructType]
    ///
    /// Example: `TodosForm`
    pub fn suffix(&self) -> &'static str {
        match self {
            StructType::Read => "",
            StructType::Update => "",
            StructType::Create => "",
        }
    }

    /// Format a struct with all prefix- and suffixes
    ///
    /// Example: `UpdateTodos`
    pub fn format(&self, name: &str) -> String {
        format!(
            "{struct_prefix}{struct_name}{struct_suffix}",
            struct_prefix = self.prefix(),
            struct_name = name,
            struct_suffix = self.suffix()
        )
    }
}

#[derive(Debug)]
struct Struct<'a> {
    /// Struct name (like `UpdateTodos`)
    identifier: String,
    /// Type of the Struct
    ty: StructType,
    /// Parsed table reference
    table: &'a ParsedTableMacro,
    /// Generation options specific for the current table
    opts: TableOptions<'a>,
    /// Global generation options
    config: &'a GenerationConfig<'a>,
    /// Storage for the finished generated code
    rendered_code: Option<String>,
    /// Cache for if this struct even has any fields
    has_fields: Option<bool>, // note: this is only correctly set after a call to render() which gets called in Struct::new()
}

#[derive(Debug, Clone)]
pub struct StructField {
    /// Name for the field
    // TODO: should this be a Ident instead of a string?
    pub name: String,
    /// Base Rust type, like "String" or "i32" or "u32"
    pub base_type: String,
    /// Indicate that this field is optional
    pub is_optional: bool,
}

impl StructField {
    /// Assemble the current options into a rust type, like `base_type: String, is_optional: true` to `Option<String>`
    pub fn to_rust_type(&self) -> Cow<str> {
        if self.is_optional {
            return format!("Option<{}>", self.base_type).into();
        }

        return self.base_type.as_str().into();
    }
}

impl From<&ParsedColumnMacro> for StructField {
    fn from(value: &ParsedColumnMacro) -> Self {
        let name = value.name.to_string();
        // convert integers to proper rust integers
        let base_type = if value.is_unsigned {
            value.ty.replace('i', "u")
        } else {
            value.ty.clone()
        };

        Self {
            name,
            base_type,
            is_optional: value.is_nullable,
        }
    }
}

impl<'a> Struct<'a> {
    /// Create a new instance
    pub fn new(
        ty: StructType,
        table: &'a ParsedTableMacro,
        config: &'a GenerationConfig<'_>,
    ) -> Self {
        let mut obj = Self {
            identifier: ty.format(table.struct_name.as_str()),
            opts: config.table(&table.name.to_string()),
            table,
            ty,
            config,
            rendered_code: None,
            has_fields: None,
        };
        obj.render();
        obj
    }

    /// Get the rendered code, or a empty string
    pub fn code(&self) -> &str {
        self.rendered_code.as_deref().unwrap_or_default()
    }

    /// Get if the current struct has fields
    ///
    /// Currently panics if [`render`](#render) has not been called yet
    pub fn has_fields(&self) -> bool {
        self.has_fields.unwrap()
    }

    fn attr_tsync(&self) -> &'static str {
        #[cfg(feature = "tsync")]
        match self.opts.get_tsync() {
            true => "#[tsync::tsync]\n",
            false => "",
        }
        #[cfg(not(feature = "tsync"))]
        ""
    }

    /// Assemble all derives for the struct
    fn attr_derive(&self) -> String {
        format!("#[derive(Debug, {derive_serde}Clone, Queryable, Insertable{derive_aschangeset}{derive_identifiable}{derive_associations}{derive_selectable}{derive_default})]",
                derive_selectable = match self.ty {
                    StructType::Read => { ", Selectable" }
                    _ => { "" }
                },
                derive_associations = match self.ty {
                    StructType::Read => {
                        if !self.table.foreign_keys.is_empty() { ", Associations" } else { "" }
                    }
                    _ => { "" }
                },
                derive_identifiable = match self.ty {
                    StructType::Read => {
                        if !self.table.foreign_keys.is_empty() { ", Identifiable" } else { "" }
                    }
                    _ => { "" }
                },
                derive_aschangeset = if self.fields().iter().all(|f| self.table.primary_key_column_names().contains(&f.name)) {""} else { ", AsChangeset" },
                derive_default = match self.ty {
                    StructType::Update => { ", Default" }
                    _ => { "" }
                },
                derive_serde = if self.config.table(&self.table.name.to_string()).get_serde() {
                    "Serialize, Deserialize, "
                } else { "" }
        )
    }

    /// Convert [ParsedColumnMacro]'s to [StructField]'s
    ///
    /// Fields filtered out:
    /// - in Create-Structs: auto-generated fields
    /// - in Update-Structs: the primary key(s)
    fn fields(&self) -> Vec<StructField> {
        self.table
            .columns
            .iter()
            .filter(|c| {
                let is_autogenerated = self
                    .opts
                    .autogenerated_columns
                    .as_deref()
                    .unwrap_or_default()
                    .contains(&c.name.to_string().as_str());

                match self.ty {
                    StructType::Read => true,
                    StructType::Update => {
                        let is_pk = self.table.primary_key_columns.contains(&c.name);

                        !is_pk
                    }
                    StructType::Create => !is_autogenerated,
                }
            })
            .map(StructField::from)
            .collect()
    }

    /// Render the full struct
    fn render(&mut self) {
        let ty = self.ty;
        let table = &self.table;

        let primary_keys: Vec<String> = table.primary_key_column_names();

        let belongs_to = table
            .foreign_keys
            .iter()
            .map(|fk| {
                format!(
                    ", belongs_to({foreign_table_name}, foreign_key={join_column})",
                    foreign_table_name = fk.0.to_string().to_pascal_case(),
                    join_column = fk.1
                )
            })
            .collect::<Vec<String>>()
            .join(" ");

        let fields = self.fields();

        if fields.is_empty() {
            self.has_fields = Some(false);
            self.rendered_code = None;
            return;
        }

        let lifetimes = {
            let lifetimes = match self.ty {
                StructType::Read => "",
                StructType::Update => self.opts.get_update_str_type().get_lifetime(),
                StructType::Create => self.opts.get_create_str_type().get_lifetime(),
            };

            if lifetimes.is_empty() {
                String::new()
            } else {
                format!("<{}>", lifetimes)
            }
        };

        let mut lines = vec![];
        for mut f in fields.into_iter() {
            let field_name = &f.name;

            if f.base_type == "String" {
                f.base_type = match self.ty {
                    StructType::Read => f.base_type,
                    StructType::Update => self.opts.get_update_str_type().as_str().to_string(),
                    StructType::Create => self.opts.get_create_str_type().as_str().to_string(),
                }
            }

            let mut field_type = f.to_rust_type();

            // always wrap a field in "Option<>" for a update struct, instead of flat options
            // because otherwise you could not differentiate between "Dont touch this field" and "Set field to null"
            // also see https://github.com/Wulf/dsync/pull/83#issuecomment-1741905691
            if self.ty == StructType::Update {
                field_type = format!("Option<{}>", field_type).into();
            }

            lines.push(format!(r#"    pub {field_name}: {field_type},"#));
        }

        let struct_code = format!(
            indoc! {r#"
            {tsync_attr}{derive_attr}
            #[diesel(table_name={table_name}{primary_key}{belongs_to})]
            pub struct {struct_name}{lifetimes} {{
            {lines}
            }}
        "#},
            tsync_attr = self.attr_tsync(),
            derive_attr = self.attr_derive(),
            table_name = table.name,
            struct_name = ty.format(&table.struct_name),
            lifetimes = lifetimes,
            primary_key = if ty != StructType::Read {
                "".to_string()
            } else {
                format!(", primary_key({})", primary_keys.join(","))
            },
            belongs_to = if ty != StructType::Read {
                "".to_string()
            } else {
                belongs_to
            },
            lines = lines.join("\n"),
        );

        self.has_fields = Some(true);
        self.rendered_code = Some(struct_code);
    }
}

/// Generate all functions (insides of the `impl StuctName { here }`)
fn build_table_fns(
    table: &ParsedTableMacro,
    config: &GenerationConfig,
    create_struct: Struct,
    update_struct: Struct,
) -> String {
    let table_options = config.table(&table.name.to_string());

    let primary_column_name_and_type: Vec<(String, String)> = table
        .primary_key_columns
        .iter()
        .map(|pk| {
            let col = table
                .columns
                .iter()
                .find(|it| it.name.to_string().eq(pk.to_string().as_str()))
                .expect("Primary key column doesn't exist in table");

            (col.name.to_string(), col.ty.to_string())
        })
        .collect();

    let item_id_params = primary_column_name_and_type
        .iter()
        .map(|name_and_type| {
            format!(
                "param_{name}: {ty}",
                name = name_and_type.0,
                ty = name_and_type.1
            )
        })
        .collect::<Vec<String>>()
        .join(", ");
    let item_id_filters = primary_column_name_and_type
        .iter()
        .map(|name_and_type| {
            format!(
                "filter({name}.eq(param_{name}))",
                name = name_and_type.0.to_string()
            )
        })
        .collect::<Vec<String>>()
        .join(".");

    // template variables
    let table_name = table.name.to_string();
    #[cfg(feature = "tsync")]
    let tsync = match table_options.get_tsync() {
        true => "#[tsync::tsync]",
        false => "",
    };
    #[cfg(not(feature = "tsync"))]
    let tsync = "";
    #[cfg(feature = "async")]
    let async_keyword = if table_options.get_async() {
        " async"
    } else {
        ""
    };
    #[cfg(not(feature = "async"))]
    let async_keyword = "";
    #[cfg(feature = "async")]
    let await_keyword = if table_options.get_async() {
        ".await"
    } else {
        ""
    };
    #[cfg(not(feature = "async"))]
    let await_keyword = "";
    let struct_name = &table.struct_name;
    let schema_path = &config.schema_path;
    let create_struct_identifier = &create_struct.identifier;
    let update_struct_identifier = &update_struct.identifier;

    let mut buffer = String::new();

    buffer.push_str(&format!(
        r##"{tsync}
#[derive(Debug, {serde_derive})]
pub struct PaginationResult<T> {{
    pub items: Vec<T>,
    pub total_items: i64,
    /// 0-based index
    pub page: i64,
    pub page_size: i64,
    pub num_pages: i64,
}}
"##,
        serde_derive = if table_options.get_serde() {
            "Serialize"
        } else {
            ""
        }
    ));

    buffer.push_str(&format!(
        r##"
impl {struct_name} {{
"##
    ));

    if create_struct.has_fields() {
        buffer.push_str(&format!(
            r##"
    pub{async_keyword} fn create(db: &mut ConnectionType, item: &{create_struct_identifier}) -> QueryResult<Self> {{
        use {schema_path}{table_name}::dsl::*;

        insert_into({table_name}).values(item).get_result::<Self>(db){await_keyword}
    }}
"##
        ));
    } else {
        buffer.push_str(&format!(
            r##"
    pub{async_keyword} fn create(db: &mut ConnectionType) -> QueryResult<Self> {{
        use {schema_path}{table_name}::dsl::*;

        insert_into({table_name}).default_values().get_result::<Self>(db){await_keyword}
    }}
"##
        ));
    }

    buffer.push_str(&format!(
        r##"
    pub{async_keyword} fn read(db: &mut ConnectionType, {item_id_params}) -> QueryResult<Self> {{
        use {schema_path}{table_name}::dsl::*;

        {table_name}.{item_id_filters}.first::<Self>(db){await_keyword}
    }}
"##
    ));

    buffer.push_str(&format!(r##"
    /// Paginates through the table where page is a 0-based index (i.e. page 0 is the first page)
    pub{async_keyword} fn paginate(db: &mut ConnectionType, page: i64, page_size: i64) -> QueryResult<PaginationResult<Self>> {{
        use {schema_path}{table_name}::dsl::*;

        let page_size = if page_size < 1 {{ 1 }} else {{ page_size }};
        let total_items = {table_name}.count().get_result(db){await_keyword}?;
        let items = {table_name}.limit(page_size).offset(page * page_size).load::<Self>(db){await_keyword}?;

        Ok(PaginationResult {{
            items,
            total_items,
            page,
            page_size,
            /* ceiling division of integers */
            num_pages: total_items / page_size + i64::from(total_items % page_size != 0)
        }})
    }}
"##));

    // TODO: If primary key columns are attached to the form struct (not optionally)
    // then don't require item_id_params (otherwise it'll be duplicated)

    // if has_update_struct {
    if update_struct.has_fields() {
        // It's possible we have a form struct with all primary keys (for example, for a join table).
        // In this scenario, we also have to check whether there are any updatable columns for which
        // we should generate an update() method.

        buffer.push_str(&format!(r##"
    pub{async_keyword} fn update(db: &mut ConnectionType, {item_id_params}, item: &{update_struct_identifier}) -> QueryResult<Self> {{
        use {schema_path}{table_name}::dsl::*;

        diesel::update({table_name}.{item_id_filters}).set(item).get_result(db){await_keyword}
    }}
"##));
    }

    buffer.push_str(&format!(
        r##"
    pub{async_keyword} fn delete(db: &mut ConnectionType, {item_id_params}) -> QueryResult<usize> {{
        use {schema_path}{table_name}::dsl::*;

        diesel::delete({table_name}.{item_id_filters}).execute(db){await_keyword}
    }}
"##
    ));

    buffer.push_str(
        r##"
}"##,
    );

    buffer
}

/// Generate all imports for the struct file that are required
fn build_imports(table: &ParsedTableMacro, config: &GenerationConfig) -> String {
    let table_options = config.table(&table.name.to_string());
    let belongs_imports = table
        .foreign_keys
        .iter()
        .map(|fk| {
            format!(
                "use {model_path}{foreign_table_name_model}::{singular_struct_name};",
                foreign_table_name_model = fk.0.to_string().to_snake_case().to_lowercase(),
                singular_struct_name = fk.0.to_string().to_pascal_case(),
                model_path = config.model_path
            )
        })
        .collect::<Vec<String>>()
        .join("\n");
    #[cfg(feature = "async")]
    let async_imports = if table_options.get_async() {
        "\nuse diesel_async::RunQueryDsl;"
    } else {
        ""
    };
    #[cfg(not(feature = "async"))]
    let async_imports = "";

    let mut schema_path = config.schema_path.clone();
    schema_path.push('*');

    let serde_imports = if table_options.get_serde() {
        "use serde::{Deserialize, Serialize};"
    } else {
        ""
    };

    let fns_imports = if table_options.get_fns() {
        "\nuse diesel::QueryResult;"
    } else {
        ""
    };

    let connection_type_alias = if table_options.get_fns() {
        format!(
            "\ntype ConnectionType = {connection_type};",
            connection_type = config.connection_type,
        )
    } else {
        "".to_string()
    };

    format!(
        indoc! {"
        use crate::diesel::*;
        use {schema_path};{fns_imports}
        {serde_imports}{async_imports}
        {belongs_imports}
        {connection_type_alias}
    "},
        belongs_imports = belongs_imports,
        async_imports = async_imports,
        schema_path = schema_path,
        serde_imports = serde_imports,
        fns_imports = fns_imports,
        connection_type_alias = connection_type_alias,
    )
    .trim_end()
    .to_string()
}

/// Generate a full file for a given diesel table
pub fn generate_for_table(table: &ParsedTableMacro, config: &GenerationConfig) -> String {
    let table_options = config.table(&table.name.to_string());
    // first, we generate struct code
    let read_struct = Struct::new(StructType::Read, table, config);
    let update_struct = Struct::new(StructType::Update, table, config);
    let create_struct = Struct::new(StructType::Create, table, config);

    let mut structs = String::new();
    structs.push_str(read_struct.code());
    structs.push('\n');
    structs.push_str(create_struct.code());
    structs.push('\n');
    structs.push_str(update_struct.code());

    let functions = if table_options.get_fns() {
        build_table_fns(table, config, create_struct, update_struct)
    } else {
        "".to_string()
    };
    let imports = build_imports(table, config);

    format!("{FILE_SIGNATURE}\n\n{imports}\n\n{structs}\n{functions}")
}
