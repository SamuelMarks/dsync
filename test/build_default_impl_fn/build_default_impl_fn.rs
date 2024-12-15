#[test]
fn test_build_default_impl_fn() {
    const CONNECTION_TYPE: &'static str = "diesel::pg::Pg";
    let config = dsync::GenerationConfig {
        connection_type: String::from(CONNECTION_TYPE),
        //diesel_backend: String::from(CONNECTION_TYPE),
        options: dsync::GenerationConfigOpts {
            table_options: std::collections::HashMap::new(),
            default_table_options: dsync::TableOptions::default(),
            schema_path: String::from("crate::schema::"),
            model_path: String::from("crate::models::"),
            once_common_structs: false,
            once_connection_type: false,
            readonly_prefixes: Vec::new(),
            readonly_suffixes: Vec::new(),
            default_impl: true,
        },
    };

    let r = dsync::generate_code(r#"
    diesel::table! {
        clients (id) {
            id -> Int4,
            redirect_uri -> Text,
            created_at -> Timestamp
        }
    }"#,
        & config).expect("CONFIG wrong");
    println!("r = {:#?}", r);
}
