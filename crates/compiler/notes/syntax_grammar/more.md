
also nothing is set in stone and i am open to ideas that can improve the designs and even any of the existing implementations. a few nuances:

query returned data shape is exclusively determined by the projection. so even limit 1, returns array or one of the document, and even `select 3 from user:User`; should return just 3 despite the scanning of entire users. the projection determines the shape and data returned and can be navigated. so it could be `select user[*].name ...`  or `select user[0].{..} from ...` or just about any valid expression and can be nested. can even just be select [1,2,6] from ...`. and you can also access any downstream table declarations in links.

for `links`, relationships are explicit via edge collections, and the most common traversal shape is:

- `(node_spec) -> [edge_spec] -> (node_spec)` (and `<-`)

however, the surface syntax may also allow a **node-to-node** hop:

- `(node_spec) -> (node_spec)`

This is not an edge-based correlation traversal; it is a convenience “attach this collection here” hop (broadcast/attachment).

Edge-to-edge hops are still disallowed.

also note: the language has no explicit join *syntax* in the query; internally a planner can still represent traversals as joins, but the user-facing semantics are “nested materialization” (virtual nested fields reachable via paths).
when you use <entire_table_collection_var@each_item>:<TableName> format in `from` or link segment, 
you can reference any of the variable and alias downstream and can reference the entire table var within projections also. 
the idea is that @alias is meant to represent each item whenever you are doing array operations like filter or order or even 
range and also within nested projection to reference each item of the aliased entire array, but if it is an object or even a scalar, 
it refers to the whole thing. although in my scan and link segment, i might have used the word alias, 
it infact is almost like a variable declaration binding to entire collection returned for the scan or segment even if limit 1 used, 
an array will be returned. @alias is responsible for binding to array item or entire object along an array/object navigation and makes 
it possible to be able to reference anywhere along the navigation within a projection or array operation without re-typing the entire 
thing over and over if you need only something along the path of the object/array already navigated. 
it is the one using LogicalBindAtExpr wrapping the expression which usually is an identifier that can refer to an entire collection, 
or object or scalar or anything else you think is reasonable. my language is designed to be expression-driven rather than statements and keywords. 
the reason i do things unlike typical relational where projection does row by row is that i want to give users more flexibility, 
freedom and intuitiveness, so that they can have access to both the entire collection scanned and also drive the iteration themselves 
via array operations like e.g 

```py
users[*].{}, or users[where ..][*].{} or users[5..8][*].name or users@u[order by u.age].name or anything like that
and this can be chained and deeply nested but similar principle applied so, 
you can have users@u[*][5..8][where u.age > 90].info@i[*].config@c[0].{title, 
reassigned_or_new_stuff_can_even_be_a_block_expr: 4 + 6, nested_normal_object: {} , nested_with_cess: u.embedded_array_or_obj[*].title } . 
so you must always use such array access/map before the records. and you can flatten using users[**] or 
even method users.flatten() but the previous of using [] in projection is more idiomatic but i support every valid expression.



// Document Access
users@u[*][5..8][where u.age > 90].info@i[*].config@c[0].{
    title,  // this is shortcut and should be same as title: c.title,
    key: c.kv,
    another: 45, // this is reassigned as it does not use `c`
}


users@u[*][5..8][where u.age > 90].info@i[*].config[0].{
    title,  // this is shortcut and should be same as title: c.title,
    key,
    another: 45, // this is reassigned as it does not use `c`
    user_name: u.name
}

// Object: does not have a base like document access
{
    title, // should be same as `title: title` since this does not have base
    key: key,
    another: 45

    }
    
```


 scoping is what we used to manage variable references and this is an entirely typed language, i have provided example of what schemas could look like in terms of definitions:


is this solution specifically tailored for my particular document-graph-like query language and considering all the code and context and syntax of my query that i have provided and understand that it is quite different from the typical relational db model with rows and columns but more of documents and e.g the projection is completely responsible for the shape of returned data in a select query, whether it's a scalar, array, object, nested or flatted or whatever, and how iterations, filters, ordering, traversing, linking, aliasing, table variables binding/declaration? 

- my language is fully typed. and aims to provide static typechecking with ide integration:


object Address {
  street: String,
  city: String,
  country: String
}

type MyType = String | Int
type MyType2 = Array<String> | Array<Int>
type MyType3 = LinkOne<Address> | LinkMany<Address>

object Contact {
  @unique
  type: Enum<'email' | 'phone'>,

  value: String,

  @default(false)
  verified: Bool
}

@permission(role = "admin", op = ["select", "insert", "update", "delete"])
@permission(role = "user", op = ["select"], when = "$self_id == user.id")
@validate(user.age >= 18 || user.parental_consent == true, "Must be 18 or have consent")
@index(name = "by_name_created", type = "btree", fields = ["name", "created"])
@index(type = "fulltext", fields = ["bio", "content"])
@table
table User {
  @primary
  id: UUID,

  @unique
  @validate(email matches regex::email(), "Invalid email")
  email: String,

  @default("Anonymous")
  name: String,

  @default(0)
  @validate(age >= 0, "Age must be non-negative")
  age: Int,

  @optional
  parental_consent: Bool,

  @index(type = "btree")
  created: Timestamp,

  @updated_at
  updated: Timestamp,

  @relation(type = "Address")
  @index(fields = ["address.city", "address.country"])
  address: link_one<Address>,
  -- address: ref<Address>,
  -- address: Address,

  @relation(type = "Contact")
  contacts: Array<Contact>,

  @permission(role = "admin", op = ["update"])
  @permission(role = "user", op = ["update"], when = "$self_id == user.id")
  phone_number: String,

  -- @index(type = "btree", fields = ["name", "created"])
  -- _compound_index_marker: (),
}


-- Renaming fields
@version("2.0.0")
@table
TABLE User {
  @rename(from: "old_username", version: "1.2")
  @alias("old_username")
  @unique
  login: String ,
  
  @rename(from: "user_age", until: "2024-06-01")
  @deprecated("Use birth_date instead")
  age: Int,
  
  birth_date: Date
}

-- Renaming table/type
@rename_table(from: "LegacyUser", version: "1.5")
TABLE Account {
  @primary
  id: UUID 
  ...
}


-- Dual-Write Mode
TABLE User {
  @rename(from: "username", mode: DUAL_WRITE)
  @sync_from(username)
  @sync(from=username, until='2024-10-21')
  login: String,
  
  @deprecated @sync_to(login)
  @sync(to=username, until='2024-10-21')
  username: String
}



TABLE User {
  @rename(from: "username", mode: DUAL_WRITE)
  @sync_from(username)
  login: String,

  @deprecated
  @sync_to(login)
  username: String,

  @derived_from("first_name + ' ' + last_name")
  full_name: String,

  @compose(["street", "city", "zip"])
  @relation(type = "Address")
  address: Address,

  @alias("user_id")
  id: UUID,
}

-- data/ migration/ Separated from schema migration
@job()
fn normalize_logins() {
  for user in User {
    if user.login.contains(" ") {
      user.login = user.login.slugify()
      save user
    }
  }
}

@cron(every = "24h", retry = 3)
fn cleanup_orphans() {
  delete from Comment where user_id not in User
}

@job(timeout="1h")
fn normalize_logins() { ... } // probbably run in a transaction or background

@cron_job(every="24h")
fn cleanup_orphans() { ... }

@hook(on_schema_change)
fn foo() { ... }

@trigger(on_field_change, table=User)
fn bar() { ... }

@once (one-shot jobs)
@on_event(table = "User", op = "insert") (triggers)
@manual, @retry, @timeout, @priority

@event
@event("user.created")
fn on_user_created(user: User) {
  // Send welcome email
}


-- TO consider

@virtual:	For computed-only fields
@materialized:	Marks field as denormalized but persisted
@ttl(...):	Time-to-live for temporal/expiring fields
@readonly

-- Experimental
@table
table User {
    id: UUID,
    email: String,
}

impl User {
  <!-- @method -->
  fn find_by_email(email: String) -> User {
  // Query or logic, e.g., "select * from user where email = $1"

  }
}
type Address {
    street: String,
    city: String,

    @method
    fn full_address() -> String {
        // return street + ", " + city
    }
}


---
i want to make sure that the planner rewrite/reordering is robust, complete and correct. i dont have explicit join syntax in my query; the point is that the engine can internally represent traversals/subqueries as join-like plan nodes and then reorder them safely (decorrelation, predicate pushdown, etc). but i want you to remember how the projection works and how this is document based and not necessarily like row/column. and the importance of schema, types scoping, almost like a normal ideal modern powerful programming language. and it is very important to me that your code always compiles ad is complete, correct and robust.


so, i want to make sure plan reordering is completely implemented correctly and robustly. and feel free to recommend any changes anywhere in the codebase if need be to improve or make new feature possible. so, outline/list everything we need to fully complete the reordering optimization and lets go one by one till we complete the implementation robustly production-ready tailored to my query language design.
