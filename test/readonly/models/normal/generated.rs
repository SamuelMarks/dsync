/* @generated and managed by dsync */

use crate::diesel::*;
use crate::schema::*;
use diesel::QueryResult;
use serde::{Deserialize, Serialize};


type ConnectionType = diesel::r2d2::PooledConnection<diesel::r2d2::ConnectionManager<diesel::PgConnection>>;

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable)]
#[diesel(table_name=normal, primary_key(id))]
pub struct Normal {
    pub id: crate::schema::sql_types::Int,
    pub testprop: crate::schema::sql_types::Int,
}

#[derive(Debug, Clone, Serialize, Deserialize, Insertable)]
#[diesel(table_name=normal)]
pub struct CreateNormal {
    pub testprop: crate::schema::sql_types::Int,
}

#[derive(Debug, Clone, Serialize, Deserialize, AsChangeset, Default)]
#[diesel(table_name=normal)]
pub struct UpdateNormal {
    pub testprop: Option<crate::schema::sql_types::Int>,
}


#[derive(Debug, Serialize)]
pub struct PaginationResult<T> {
    pub items: Vec<T>,
    pub total_items: i64,
    /// 0-based index
    pub page: i64,
    pub page_size: i64,
    pub num_pages: i64,
}

impl Normal {

    pub fn create(db: &mut ConnectionType, item: &CreateNormal) -> QueryResult<Self> {
        use crate::schema::normal::dsl::*;

        insert_into(normal).values(item).get_result::<Self>(db)
    }

    pub fn read(db: &mut ConnectionType, param_id: crate::schema::sql_types::Int) -> QueryResult<Self> {
        use crate::schema::normal::dsl::*;

        normal.filter(id.eq(param_id)).first::<Self>(db)
    }

    /// Paginates through the table where page is a 0-based index (i.e. page 0 is the first page)
    pub fn paginate(db: &mut ConnectionType, page: i64, page_size: i64) -> QueryResult<PaginationResult<Self>> {
        use crate::schema::normal::dsl::*;

        let page_size = if page_size < 1 { 1 } else { page_size };
        let total_items = normal.count().get_result(db)?;
        let items = normal.limit(page_size).offset(page * page_size).load::<Self>(db)?;

        Ok(PaginationResult {
            items,
            total_items,
            page,
            page_size,
            /* ceiling division of integers */
            num_pages: total_items / page_size + i64::from(total_items % page_size != 0)
        })
    }

    pub fn update(db: &mut ConnectionType, param_id: crate::schema::sql_types::Int, item: &UpdateNormal) -> QueryResult<Self> {
        use crate::schema::normal::dsl::*;

        diesel::update(normal.filter(id.eq(param_id))).set(item).get_result(db)
    }

    pub fn delete(db: &mut ConnectionType, param_id: crate::schema::sql_types::Int) -> QueryResult<usize> {
        use crate::schema::normal::dsl::*;

        diesel::delete(normal.filter(id.eq(param_id))).execute(db)
    }

}