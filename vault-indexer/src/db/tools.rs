use super::error::Error;
use sqlite::{Bindable, Connection, ReadableWithIndex, State, Statement};

/// Contains shared operations over the database that is used in higher order
/// modules for database.
pub trait BasicDatabase {
    // Helper to execute inserts, updates and deletes (etc) that should be done in one step with no return results
    fn single_execute<T, U>(&self, tag: &str, query: &str, binds: T) -> Result<(), Error>
    where
        T: IntoIterator<Item = U>,
        U: Bindable;
}

impl BasicDatabase for Connection {
    fn single_execute<T, U>(&self, tag: &str, query: &str, binds: T) -> Result<(), Error>
    where
        T: IntoIterator<Item = U>,
        U: Bindable,
    {
        let mut statement = self.prepare(query).map_err(Error::PrepareQuery)?;

        statement.bind_iter(binds).map_err(Error::BindQuery)?;

        if let State::Done = statement.next().map_err(Error::QueryNextRow)? {
            Ok(())
        } else {
            Err(Error::ShouldExecuteOneRow(tag.to_owned()))
        }
    }
}

// Helper trait to simplify reading fields from statement and use self syntax
pub trait ReadField {
    fn read_field<T: ReadableWithIndex>(&self, name: &str) -> Result<T, Error>;
}

impl<'c> ReadField for Statement<'c> {
    fn read_field<T: ReadableWithIndex>(&self, name: &str) -> Result<T, Error> {
        let val = self
            .read::<T, _>(name)
            .map_err(|e| Error::ReadField(name.to_owned(), e))?;
        Ok(val)
    }
}
