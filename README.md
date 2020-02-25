# asn1rs - ASN.1 Compiler for Rust

This crate allows one to generate Rust, Protobuf and SQL code from ASN.1 definitions, providing also support for basic [serde](https://crates.io/crates/serde) integration.
The crate can be used as standalone binary using its command line interface or included invoked through its API
(for example inside the ```build.rs``` script).

The generated Rust code has serializers and deserializers for ASN.1 UPER, protobuf and PostgreSQL and can therefore communicate with other applications supporting these formats (like Java-classes generated by the Google protobuf compiler).

Currently only UPER is supported for ASN.1. 

## CLI usage

It is always helpful to check ```asn1rs --help``` in advance.
The basic usage can be seen blow:

```
asn1rs -t rust directory/for/rust/files some.asn1 messages.asn1
```

```
asn1rs -t proto directory/for/protobuf/files some.asn1 messages.asn1
```

```
asn1rs -t sql directory/for/sql/schema/files some.asn1 messages.asn1
```

## API usage

The following example generates Rust, Protobuf and SQL files for all ```.asn1```-files in the ```asn/``` directory of the project.
While the generated Rust code is written to the ```src/``` directory, the Protobuf files are written to ```proto/``` and the SQL files are written to ```sql/ ```.
Additionally, in this example each generated Rust-Type also receives ```Serialize``` and ```Deserialize``` derive directives (```#[derive(Serialize, Deserialize)]```) for automatic [serde](https://crates.io/crates/serde) integration.

File ```build.rs```:

```rust
extern crate asn1rs;

use std::fs;

use asn1rs::converter::convert_to_proto;
use asn1rs::converter::convert_to_rust;
use asn1rs::converter::convert_to_sql;
use asn1rs::gen::rust::RustCodeGenerator;

pub fn main() {
    for entry in fs::read_dir("asn").unwrap().into_iter() {
        let entry = entry.unwrap();
        let file_name = entry.file_name().into_string().unwrap();
        if file_name.ends_with(".asn1") {
            if let Err(e) = convert_to_rust(
                entry.path().to_str().unwrap(),
                "src/",
                |generator: &mut RustCodeGenerator| {
                    generator.add_global_derive("Serialize");
                    generator.add_global_derive("Deserialize");
                },
            ) {
                panic!("Conversion to rust failed for {}: {:?}", file_name, e);
            }
            if let Err(e) = convert_to_proto(entry.path().to_str().unwrap(), "proto/") {
                panic!("Conversion to proto failed for {}: {:?}", file_name, e);
            }
            if let Err(e) = convert_to_sql(entry.path().to_str().unwrap(), "sql/") {
                panic!("Conversion to sql failed for {}: {:?}", file_name, e);
            }
        }
    }
}
```

## Example Input / Output

Input ```input.asn1```

```asn
MyMessages DEFINITIONS AUTOMATIC TAGS ::=
BEGIN

Header ::= SEQUENCE {
    timestamp    INTEGER (0..1209600000)
}

END
```

Output ```my_messages.rs```:

```rust
// use ...

#[derive(Default, Debug, Clone, PartialEq, Hash, Serialize, Deserialize)]
pub struct Header {
    pub timestamp: u32,
}

impl Header {
    pub fn timestamp_min() -> u32 {
        0
    }

    pub fn timestamp_max() -> u32 {
        1_209_600_000
    }

    pub async fn apsql_retrieve_many(context: &apsql::Context<'_>, ids: &[i32]) -> Result<Vec<Self>, apsql::Error> { /*..*/ }
    pub async fn apsql_retrieve(context: &apsql::Context<'_>, id: i32) -> Result<Self, apsql::Error> { /*..*/ }
    pub async fn apsql_load(context: &apsql::Context<'_>, row: &apsql::Row) -> Result<Self, apsql::Error> { /*..*/ }
    pub async fn apsql_insert(&self, context: &apsql::Context<'_>) -> Result<i32, apsql::PsqlError> { /*..*/ }
}

// Serialize and deserialize functions for ASN.1 UPER
impl Uper for Header { /*..*/ }

// Serialize and deserialize functions for protobuf
impl ProtobufEq for Header { /*..*/ }
impl Protobuf for Header { /*..*/ }

// Insert and query functions for PostgreSQL
impl PsqlRepresentable for Header { /*..*/ }
impl PsqlInsertable for Header { /*..*/ }
impl PsqlQueryable for Header { /*..*/ }
```

Output ```my_messages.proto```:

```proto
syntax = 'proto3';
package my.messages;

message Header {
    uint32 timestamp = 1;
}
```

Output ```my_messages.sql```:

```sql
DROP TABLE IF EXISTS Header CASCADE;

CREATE TABLE Header (
    id SERIAL PRIMARY KEY,
    timestamp INTEGER NOT NULL
);
```

### Example usage of async postgres
NOTE: This requires the `async-psql` feature
```rust
use asn1rs::io::async_psql::*;
use tokio_postgres::NoTls;

#[tokio::main]
async fn main() {
    let (mut client, connection) = tokio_postgres::connect(
        "host=localhost user=postgres application_name=psql_async_demo",
        NoTls,
    )
        .await
        .expect("Failed to connect");

    tokio::spawn(connection);

    let transaction = client
        .transaction()
        .await
        .expect("Failed to open a new transaction");

    let context = Cache::default().into_context(transaction);

    // using sample message from above
    let message = Header {
        timestamp: 1234,
    };
   
    // This issues all necessary insert statements on the given Context and
    // because it does not require exclusive access to the context, you can
    // issue multiple inserts and await them concurrently with for example
    // tokio::try_join, futures::try_join_all or the like. 
    let id = message.apsql_insert(&context).await.expect("Insert failed");
    
    // This disassembles the context, allowing the Transaction to be committed
    // or rolled back. This operation also optimizes the read access to
    // prepared statements of the Cache. If you do not want to do that, then call
    // Context::split_unoptimized instead.
    // You can also call `Cache::optimize()` manually to optimize the read access
    // to the cached prepared statements.
    // See the doc for more information about the usage of cached prepared statements
    let (mut cache, transaction) = context.split();
   
    transaction.commit().await.expect("failed to commit");

    let transaction = client
        .transaction()
        .await
        .expect("Failed to open a new transaction");

    let context = cache.into_context(transaction);
    let message_from_db = Header::apsql_retrieve(&context, id).await.expect("Failed to load");

    assert_eq!(message, message_from_db);
}
```

## Good to know
The module ```asn1rs::io``` exposes (de-)serializers and helpers for direct usage without ASN.1 definitons:
```rust
use asn1rs::io::uper::*;
use asn1rs::io::buffer::BitBuffer;

let mut buffer = BitBuffer::default();
buffer.write_bit(true).unwrap();
buffer.write_utf8_string("My UTF8 Text").unwrap();

send_to_another_host(buffer.into::<Vec<u8>>()):
```
```rust
use asn1rs::io::protobuf::*;

let mut buffer = Vec::default();
buffer.write_varint(1337).unwrap();
buffer.wrote_string("Still UTF8 Text").unwrap();

send_to_another_host(buffer):
``` 

## What works
 - Generating Rust Code with serializtion support for
   - UPER
   - Protobuf
   - PostgreSQL
   - async PostgreSQL
 - Generating Protobuf Definitions
 - Generating PostgreSQL Schema files
 - Support for the following ASN.1 datatypes:
   - ```SEQUENCE```, ```SEQUENCE OF```, ```CHOICE``` and ```ENUMERATED```
   - inline ```SEQUENCE OF``` and ```CHOICE``` 
   - ```OPTIONAL```
   - ```INTEGER``` with range value only (numbers or ```MIN```/```MAX```)
   - ```UTF8String```
   - ```OCTET STRING``` 
   - ```BOOLEAN```
   - using previously declared message types
   - ```IMPORTS .. FROM ..;```  
   

## What doesn't work
 - most of the (not mentioned) remaining ASN.1 data-types
 - probably most non-trivial ASN.1 declarations
 - comments
 - let me know

## TODO
Things to do at some point in time
  - support ```#![no_std]```
  - refactor / clean-up (rust) code-generators


### LICENSE

MIT/Apache-2.0

### About
This crate was initially developed during a research project at IT-Designers GmbH (http://www.it-designers.de).
