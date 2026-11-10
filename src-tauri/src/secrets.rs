use std::collections::HashMap;

use disweep_core::Secret;
use keyring::{Entry, Error};
use parking_lot::Mutex;

const SERVICE: &str = "dev.vante.disweep";
pub const BOT_KEY: &str = "bot";

#[must_use]
pub fn account_key(id: disweep_core::Snowflake) -> String {
    format!("account:{id}")
}

pub enum Secrets {
    Keychain,
    Memory(Mutex<HashMap<String, Secret>>),
}

fn describe(error: &Error) -> String {
    format!("Your system's credential store refused this: {error}")
}

impl Secrets {
    pub fn save(&self, key: &str, secret: &Secret) -> Result<(), String> {
        match self {
            Self::Keychain => {
                let entry = Entry::new(SERVICE, key).map_err(|error| describe(&error))?;
                entry
                    .set_password(secret.expose())
                    .map_err(|error| describe(&error))
            }
            Self::Memory(store) => {
                store.lock().insert(key.to_owned(), secret.clone());
                Ok(())
            }
        }
    }

    pub fn load(&self, key: &str) -> Result<Option<Secret>, String> {
        match self {
            Self::Keychain => {
                let entry = Entry::new(SERVICE, key).map_err(|error| describe(&error))?;
                match entry.get_password() {
                    Ok(value) => Ok(Some(Secret::new(value))),
                    Err(Error::NoEntry) => Ok(None),
                    Err(error) => Err(describe(&error)),
                }
            }
            Self::Memory(store) => Ok(store.lock().get(key).cloned()),
        }
    }

    pub fn delete(&self, key: &str) -> Result<(), String> {
        match self {
            Self::Keychain => {
                let entry = Entry::new(SERVICE, key).map_err(|error| describe(&error))?;
                match entry.delete_credential() {
                    Ok(()) | Err(Error::NoEntry) => Ok(()),
                    Err(error) => Err(describe(&error)),
                }
            }
            Self::Memory(store) => {
                store.lock().remove(key);
                Ok(())
            }
        }
    }
}
