# dbdrill

*Navigate the entities in your database at the speed of light ⚡️*

![dbdrill screencast](docs/screencast.gif)

There are plenty of good UIs to manage SQL databases. There are lots of good
tools to explore SQL schemas. Dbdrill is something else. It allows you to:

- efficiently list entities using predefined queries
- for a given entity, interactively explore related entities using predefined
  relations

If that sounds a bit abstract, imagine for example "find the user with email
foo@example.com, show the blogs where this user has edit rights, then show the
posts for that blog".

ℹ️ dbdrill is early stage software. It won't eat your data because it only runs
`SELECT` queries, but various SQL data types are not supported yet and some
features may be incomplete.

## Installation

### From source

Run `cargo run` to build and run dbdrill.

### Using Nix

You can run dbdrill using `nix run 'github:abustany/dbdrill'`

Alternatively, you can use `github:abustany/dbdrill` as a flake input.

## User guide

dbdrill requires:

- a SQL database (for now only PostgreSQL is supported, but adding support for
  other engines would be easy)
- a TOML configuration file describing the entities in your database, and the
  links between those entities

This user guide will walk through the few concepts of dbdrill, using a sample
database as an example. You can find the database dump in
[docs/sample-db.sql](docs/sample-db.sql) and the dbdrill configuration file in
[docs/dbdrill.toml](docs/dbdrill.toml).

### Entities

I've used the word "entity" several times above. For dbdrill, an entity is
anything that can be fetched as an SQL row, no matter if it comes from a single
table or a complex join.

Entities are defined using toplevel sections in the dbdrill configuration file,
and only need a name.

For example, to define an entity called user:

```toml
[user]
name = "User"
```

#### Listing entities

Entities are not much use if you can't search for them. Dbdrill allows defining
an arbitrary number of "searches" for an entity. For example, you might want to
look up a user using its unique ID, its email address, or doing a fuzzy search
on their name. Searches are basically parameterized bookmarked queries to list
entities. Searches can return an arbitrary number of rows.

Searches are defined in the `search` section of an entity.

For example, to define some searches on our user entity from above:

```toml
[user.search.id]
# Assuming our users table has an id column of type SERIAL
query = "SELECT * FROM users WHERE id = $1" # PostgreSQL uses $n for placeholders
params = [{name = "ID", type = "integer"}] # params describes the placeholders in the query

[user.search.email]
query = "SELECT * FROM users WHERE email = $1"
params = [{name = "Email"}] # type = "text" is default if not specified

[user.search.name]
query = "SELECT * FROM users WHERE name ILIKE $1"
params = [{name = "Name pattern"}]
```

If you launch dbdrill, you'll now be able to choose "User" in the entity
picker, and search users using various criteria.

Assuming your configuration file is called `dbdrill.toml`, and your PostgreSQL
database is running on localhost:

```
# The database DSN can also be passed using the DB_DSN environment variable
dbdrill --db-dsn 'postgres://user:password@localhost:5432/mydb' dbdrill.toml
```

#### Linking entities

Dbdrill allows you to describe how entities are linked together in your
database. Links rely entirely on the dbdrill configuration file, and do not
depend on database level constructs like foreign keys.

A link has both a source and a target entity, and defines how for a given
source entity you can search for target entities. For example, how for a user
you can search for blogs where this user has edit rights.

Links are defined in the `links` section of an entity, and describe how to bind
the data of a source entity to the parameters of search on the target entity.

Let's continue with the blogging platform example. We'll assume that next to
our `users` table we also have a `blogs` table that lists blogs, and a
`user_blogs` table that describes users privileges on each blog. We'll first
allow listing blogs for a given user with editor privileges:

```toml
[blog]
name = "Blog"

[blog.search.editor] # Search to list all blogs where a user is editor
query = "SELECT b.* FROM blogs b JOIN user_blogs ub ON (b.id = ub.blog_id) WHERE ub.user_id = $1 AND ub.role = 'editor'"
params = [{name = "User ID", type = "integer"}]
```

And we can now link users with blogs they can edit:

```toml
[user.links."Blogs"]
kind = "blog" # the ID of the target entity is the TOML section name
search = "editor" # this is the name of the search we defined on blog
search_params = ["id"] # use the "id" column of our user to fill the search parameter
```

When listing users in dbdrill, you can now press the <kbd>l</kbd> key to bring
the link picker up, choose "Blogs" and see the list of blogs appear ✨ At any
time, press the <kbd>Escape</kbd> key to go back.

Dbdrill also supports extracting data from JSONB columns when defining links.
Let's now assume that we modeled our blog-post relations in the database using
a JSONB column called `posts` on our `blogs` table. That column holds an array
of JSON objects, each with a `postId` property referring to the `id` column in
our `posts` table.

*Note: modeling things this way is probably not a good idea, but serves our
demonstration purpose*

We can tell dbdrill to use a [JSONPath](https://en.wikipedia.org/wiki/JSONPath)
expression to extract the values of the search parameters:

```toml
[post]
name = "Post"

[post.search.ids]
query = "SELECT * FROM posts WHERE id = ANY($1)"
params = [{name = "IDs", type = "integer[]"}]

[blog.links."Posts"]
kind = "post"
search = "ids"
search_params = [{json_path = [
  "posts", # Take the value of the posts column...
  "$[*].postId" # and for each entry of the array, extract the postId field
]}]
```

That's it, you've now seen everything that dbdrill can do! Check the list of
keyboard shortcuts below to make sure you're as fast as possible, and happy
exploring ⛵️

### Keyboard shortcuts

There are two global keyboard shortcuts in dbdrill:

- <kbd>Escape</kbd> goes back to the previous view
- <kbd>q</kbd> quits

When listing entities:

- <kbd>Enter</kbd> opens a popup showing the full (untruncated) values
- <kbd>l</kbd> to bring up the link picker.

Dbdrill is designed to be efficiently navigated with the keyboard. Every time
you need to pick an item in a list, you'll see a letter highlighted in each
item: press that letter to select this item directly.
