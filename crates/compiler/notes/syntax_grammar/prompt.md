#file:src #file:syntax #file:Cargo.toml 
Make a comprehensive outline/TODOs of what needs to be done to get my database query language to execute including the percentage progress indication and put these in a file. this is intended to be updated by you as you progress.

I intend to build my own in-memory, file-based storage similar to rocksdb and the likes supporting lsm/btree+ or any other good ones, distributed storage engine all with support for transaction. and perhaps, I'll support all these other ones like rocksdb, foundationdb, tikv but I want to build mine. And i also intend to build a distributed object storage and event bus which i guess will use some implementations or potentially share from some of these or reuse? But i want to focus on the database for now. And perhaps we can focus on in-memory first to get query executions right before building the lsm/btree+ or whatever based storage engine.

I want you to make the TODO to get to 100% of making the entire thhing work.

Also, for my type-system, I want a very expressive and statically checked type system. I want a typesystem that supports this level or better/robust type inference system with type unification, and also union type, generics, generics constraint, easy-to-use and understand module system with defaults types in the prelude. check #file:type_system_wants.md file for inspiration where i use type unification. I will also. also check the files under #syntax directory for more information. I have done from the AST->hir->lir->pir although not complete  and not set in stone, i am in the process of implementing/completing the execution so that i can at least get some simple to complex select queries to execute so that i can see how things come together and proceed from there.




---

i am not using LogicalExpr. I probably shpuld remove that. I just use the semanitcally resolved Expr with ExprId from the high level intermediate Representation HIR phase. it is use across from that point on and found in the hir Context. for the part that changes e.g subquery - Expr::Query(..), I map those during conversion phases e.g to lir, to pir and during execution. nothing in set in stone. just do what's robust and best.

also, not that my select queries dont work like normal relational select queries. check files in #file:syntax for inspiration like #file:select.md 
#file:complex_query.sql #file:many-examples.sql #file:more.md #file:context.md 

you can proceed but make sure you check the code reguarlary to make sure things still work and compile so too much errors dont pile up. should we do the type unification or query execution first?



----

before we proceed to making the execution working fully and being able to return results that conforms to the shape of the projection ExprId? Should we make the type inference more robust first with type unification or make the execution work first? Make a todo  for what's next with a progress percentage indication so that we can continue updating the file as we proceed to finish that feature



----


can you make sure the tests pass so that we can get execution working?

MY though process about select query, is that links represent a path from source downstream e.g with users@u writes books, one can access writes and books directly from `users` within projection and books can also be accessed indirectly via writes through a flattening navigation process e.g users@u[*]. {book_stuf: u.writes@w[*].books@b[**].{ date: w.date, name: b.name, title }}.


 and one is able to access the entire collection returned while still being able to utilize item variable in e.g iteration just like a map/filter array function. e.g users@u[where u.name == 'green'];
 benefit of this conceptually(at leas i am thinking) is that one is able to have access to the entire collection and each item just like one would in a normal programming language unlike how typical(basicall every) database language treats the two as same and have to resort to tricky syntax for accessing all or doing windowing e.g instread of something like `count(*)`, my language would refernce the collection or nested array directly e.g count(users)
or even somethign count(users[..55])
or count(users@u[where u.age > 5])

 or users@u[*].{food_count: count(u.foodlist[*] }) 

and when the base is non array, `u` would stand for the entire base value, but when an array, it represents each value as one iterates though.

Can we realistically achiece these while still being able to have performant queries?

i want us to fix the errors and make execution work with these ideas.


---

i think HirBuilder, LirBuilder, PirBuilder dont exist as you're expecting them. perhaps, what we can do is if you think that some of the apis or implementations around hir,lir,pir should be changed to be robust, feel free to make such changes you deem fit. check things like builder files, context, visitor, in hir, lir, pir folders and also the symbol table in hir.

I say this cos it seems you expect a different api in the compile_query function  in the and I am open to you creating such APIs and making other changes you deem fit, also if you want to use the lifetime approach or not e.g smart pointers. just be robust and do what's best from first principles and more production-ready. but make sure you keep checking the code so you dont pile up too much errors that become too difficult to fix them all.


---

good job but none of the tests test the execution and makes sure the right result is returned? is the structure with lifetime and using Rc the best way to go for all the contexts across hir, lir and pir or do you want to refactor the way the code are structured acrossboard especially the data structure? if so, should we do this first or makes surre the execution actually works first at least for some simple and more complex cases with links and filters