# semantics (raw notes)

this file is historical brainstorming. the canonical semantics are in:

- src/syntax/semantics.md
- src/syntax/context.md

---

this is the way i have thought about it and i am open to ideas.

when u do users@u[where u.age >5],
it's like users.map(|u| u.age > 5).

when u do users@u[0],
it's like users@u[0]

and you can even get nested ideas of users by navigating along the path e.g

users[*].tags@t[*];
perhaps this would be something like:
users.map(|u| u.tags.map(|t| {}))

or

users[*].tags@t[**];
perhaps this would be something like:
users.map(|u| u.tags.flat_map(|t| {}))

---
so, `.field` selects one field and navigates into it.

`.{a, b, reassign: 5}` selects/projects multiple fields from the object

it means downstream would be able to access the  main array and also its item variable/binding depending on how it's written and if within the scope.

I was thinking that both the main array and item users@u from earlier path in from and links would be available downstream a link or subsequent lins depending on where next one decides to continue from any of the previous.

so that both earlier main array and item  can be used downstream but not sure if i yet allow items from scan and links to be available but i allow the main arrays variable of the scanned tables/whatever.

the reason is so that besides accessing earlier object, users can also answer questions about the earlier arrays in scope without resorting to a dsl.

e.g first 3 users 
users[0..3] or users[0..=2]?

havent really settled on the range query syntax in this context yet but the general idea applies.

u can also do things like math::count(users).
or even sum(users)/len(users);

also i am thinking downstream array objects should be nested fields to upstream array objects. not sure yet if this is the best way as it may collide or override upstreams original fields but then users are allowed to use any labels. Labels are like variables basically and @u just like binding to the array item or object if the variable is an object.

And i have also shifted to using plurals for array labels for convention and easy understanding:

so, elg:


----

whichever is the most robust and intuitive. Also, especially if the approach does not necessarily have to determine/lockin the execution model efficiency.

e.g we wil be supporting async and streaming of each top level materialized/aggregated item if not flattened and if flattened, we stream eat each flattened level rate e.g without waiting for nested to fully materialize but these are mainly about executions not syntax/type semantics
