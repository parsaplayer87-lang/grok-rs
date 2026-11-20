use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use redb::{Database, ReadableTable, TableDefinition};

use crate::config::Settings;
use crate::types::{Conversation, Memory, Message};

const CONVERSATIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("conversations");
const MESSAGES: TableDefinition<&str, &[u8]> = TableDefinition::new("messages");
const MEMORIES: TableDefinition<&str, &[u8]> = TableDefinition::new("memories");
const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");

/// Message keys are `conversation_id/message_id` so a whole conversation is one
/// ordered range scan instead of a rewritten index blob.
fn msg_key(conv: &str, id: &str) -> String {
    format!("{conv}/{id}")
}

fn msg_range(conv: &str) -> (String, String) {
    (format!("{conv}/"), format!("{conv}0"))
}

#[derive(Clone)]
pub struct Store {
    db: Arc<Database>,
}

impl Store {
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating data dir {}", dir.display()))?;
        let db = Database::create(dir.join("grok-rs.redb")).context("opening redb database")?;
        let store = Self { db: Arc::new(db) };
        store.init_tables()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let db = Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .context("opening in-memory redb")?;
        let store = Self { db: Arc::new(db) };
        store.init_tables()?;
        Ok(store)
    }

    fn init_tables(&self) -> Result<()> {
        let tx = self.db.begin_write()?;
        {
            let _ = tx.open_table(CONVERSATIONS)?;
            let _ = tx.open_table(MESSAGES)?;
            let _ = tx.open_table(MEMORIES)?;
            let _ = tx.open_table(META)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn settings(&self) -> Result<Settings> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(META)?;
        match t.get("settings")? {
            Some(g) => Ok(serde_json::from_slice(g.value())?),
            None => Ok(Settings::default()),
        }
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        let bytes = serde_json::to_vec(s)?;
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(META)?;
            t.insert("settings", bytes.as_slice())?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn put_conversation(&self, c: &Conversation) -> Result<()> {
        let bytes = serde_json::to_vec(c)?;
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(CONVERSATIONS)?;
            t.insert(c.id.as_str(), bytes.as_slice())?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn conversation(&self, id: &str) -> Result<Option<Conversation>> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(CONVERSATIONS)?;
        match t.get(id)? {
            Some(g) => Ok(Some(serde_json::from_slice(g.value())?)),
            None => Ok(None),
        }
    }

    pub fn conversations(&self) -> Result<Vec<Conversation>> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(CONVERSATIONS)?;
        let mut out = Vec::new();
        for row in t.iter()? {
            let (_, v) = row?;
            out.push(serde_json::from_slice::<Conversation>(v.value())?);
        }
        out.sort_by_key(|a| std::cmp::Reverse(a.modified_at));
        Ok(out)
    }

    pub fn delete_conversation(&self, id: &str) -> Result<()> {
        let tx = self.db.begin_write()?;
        {
            let mut ct = tx.open_table(CONVERSATIONS)?;
            ct.remove(id)?;
            let mut mt = tx.open_table(MESSAGES)?;
            let (lo, hi) = msg_range(id);
            let keys: Vec<String> = mt
                .range(lo.as_str()..hi.as_str())?
                .filter_map(|r| r.ok().map(|(k, _)| k.value().to_string()))
                .collect();
            for k in keys {
                mt.remove(k.as_str())?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn put_message(&self, m: &Message) -> Result<()> {
        let bytes = serde_json::to_vec(m)?;
        let key = msg_key(&m.conversation_id, &m.id);
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(MESSAGES)?;
            t.insert(key.as_str(), bytes.as_slice())?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn message(&self, conv: &str, id: &str) -> Result<Option<Message>> {
        let key = msg_key(conv, id);
        let tx = self.db.begin_read()?;
        let t = tx.open_table(MESSAGES)?;
        match t.get(key.as_str())? {
            Some(g) => Ok(Some(serde_json::from_slice(g.value())?)),
            None => Ok(None),
        }
    }

    pub fn messages(&self, conv: &str) -> Result<Vec<Message>> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(MESSAGES)?;
        let (lo, hi) = msg_range(conv);
        let mut out = Vec::new();
        for row in t.range(lo.as_str()..hi.as_str())? {
            let (_, v) = row?;
            out.push(serde_json::from_slice::<Message>(v.value())?);
        }
        out.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        Ok(out)
    }

    pub fn delete_message(&self, conv: &str, id: &str) -> Result<()> {
        let key = msg_key(conv, id);
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(MESSAGES)?;
            t.remove(key.as_str())?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn put_memory(&self, m: &Memory) -> Result<()> {
        let bytes = serde_json::to_vec(m)?;
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(MEMORIES)?;
            t.insert(m.id.as_str(), bytes.as_slice())?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn memories(&self) -> Result<Vec<Memory>> {
        let tx = self.db.begin_read()?;
        let t = tx.open_table(MEMORIES)?;
        let mut out = Vec::new();
        for row in t.iter()? {
            let (_, v) = row?;
            out.push(serde_json::from_slice::<Memory>(v.value())?);
        }
        out.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        Ok(out)
    }

    pub fn delete_memory(&self, id: &str) -> Result<()> {
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(MEMORIES)?;
            t.remove(id)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn search_messages(&self, query: &str) -> Result<Vec<Message>> {
        let q = query.to_lowercase();
        if q.trim().is_empty() {
            return Ok(Vec::new());
        }
        let tx = self.db.begin_read()?;
        let t = tx.open_table(MESSAGES)?;
        let mut out = Vec::new();
        for row in t.iter()? {
            let (_, v) = row?;
            let m: Message = serde_json::from_slice(v.value())?;
            if m.text.to_lowercase().contains(&q) {
                out.push(m);
            }
        }
        out.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        out.truncate(200);
        Ok(out)
    }

    pub fn export_conversation(&self, conv: &str) -> Result<serde_json::Value> {
        let c = self.conversation(conv)?;
        let m = self.messages(conv)?;
        Ok(serde_json::json!({ "conversation": c, "messages": m }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{new_id, Sender};

    fn message(conv: &str, text: &str) -> Message {
        Message {
            id: new_id(),
            conversation_id: conv.to_string(),
            sender: Sender::User,
            text: text.to_string(),
            created_at: crate::types::now_ms(),
            ..Default::default()
        }
    }

    #[test]
    fn roundtrip_settings_and_conversations() -> Result<()> {
        let s = Store::open_in_memory()?;
        let settings = Settings {
            language: "fa".into(),
            ..Default::default()
        };
        s.save_settings(&settings)?;
        assert_eq!(s.settings()?.language, "fa");

        let c = Conversation {
            id: new_id(),
            title: "test".into(),
            created_at: 1,
            modified_at: 2,
            ..Default::default()
        };
        s.put_conversation(&c)?;
        assert_eq!(s.conversation(&c.id)?.unwrap().title, "test");
        assert_eq!(s.conversations()?.len(), 1);
        Ok(())
    }

    #[test]
    fn messages_are_scoped_to_their_conversation() -> Result<()> {
        let s = Store::open_in_memory()?;
        let a = new_id();
        let b = new_id();
        s.put_message(&message(&a, "alpha"))?;
        s.put_message(&message(&a, "beta"))?;
        s.put_message(&message(&b, "other"))?;
        assert_eq!(s.messages(&a)?.len(), 2);
        assert_eq!(s.messages(&b)?.len(), 1);
        assert_eq!(s.messages(&new_id())?.len(), 0);
        Ok(())
    }

    #[test]
    fn deleting_conversation_removes_only_its_messages() -> Result<()> {
        let s = Store::open_in_memory()?;
        let a = new_id();
        let b = new_id();
        s.put_conversation(&Conversation {
            id: a.clone(),
            ..Default::default()
        })?;
        s.put_message(&message(&a, "x"))?;
        s.put_message(&message(&b, "y"))?;
        s.delete_conversation(&a)?;
        assert_eq!(s.messages(&a)?.len(), 0);
        assert_eq!(s.messages(&b)?.len(), 1);
        assert!(s.conversation(&a)?.is_none());
        Ok(())
    }

    #[test]
    fn persian_text_survives_storage() -> Result<()> {
        let s = Store::open_in_memory()?;
        let c = new_id();
        s.put_message(&message(&c, "سلام دنیا 🌍"))?;
        assert_eq!(s.messages(&c)?[0].text, "سلام دنیا 🌍");
        assert_eq!(s.search_messages("سلام")?.len(), 1);
        Ok(())
    }
}
