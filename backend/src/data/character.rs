use std::{collections::HashMap, iter};

use crate::util::types::Character;

/// Records a character's name, adding it if the waitlist hasn't seen it before
pub async fn upsert(db: &crate::DB, id: i64, name: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO \"character\" (id, name) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET name = $2",
    )
    .bind(id)
    .bind(name)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn lookup(db: &crate::DB, ids: &[i64]) -> Result<HashMap<i64, Character>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    // let placeholders = iter::repeat("?")
    //     .take(ids.len())
    //     .collect::<Vec<_>>()
    //     .join(",");

    let placeholders = (1..=ids.len())
        .map(|i| "$".to_string() + &i.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let query_str = format!(
        "
        SELECT id, name, COALESCE(\"alt_character\".account_id, id) AS account_id
        FROM \"character\"
        LEFT JOIN \"alt_character\" ON id = \"alt_character\".alt_id
        WHERE id IN ({})",
        placeholders
    );
    let mut query = sqlx::query_as::<_, CharacterRecord>(&query_str);

    #[derive(Debug, sqlx::FromRow)]
    struct CharacterRecord {
        id: i64,
        name: String,
        account_id: i64,
    }
    for id in ids {
        query = query.bind(id);
    }
    Ok(query
        .fetch_all(db)
        .await?
        .into_iter()
        .map(|record| {
            (
                record.id,
                Character {
                    id: record.id,
                    name: record.name,
                    account_id: Some(record.account_id),
                },
            )
        })
        .collect())
}
