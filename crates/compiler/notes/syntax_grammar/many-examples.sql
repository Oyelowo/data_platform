-- Query 1: Basic Node Query

```graphql
select users@u[*].{
  user_id: u.id,
  name,
  age
}
from users:User;
```

-- selects a specific field in an array

```graphql
select users@u[*].name
from users:User;
```

```graphql
select users@u[0].name
from users:User;
```

```graphql
select users@u[-2].name
from users:User;
```

```graphql
select users@u[5:8].name
from users:User;
```

```graphql
select users@u[where u.name == "John"].name
from users:User;
```

```graphql
select users@u[0].{
  user_id: u.id,
  name,
  age
}
from users:User:123;
```

-- Query 2: Single Relationship Traversal

```graphql
select users@u[*].{
  user_id: u.id,
  name,
  universities: u.studies_at_rel@sta[*].universities_node[**].{ // universities_node is the alias for University items
    start_date: sta.date,
    name,
    type
  }
}
from users:User
links
  (users) -> [studies_at_rel:UserStudiesAtUniversity] -> (universities_node:University);
```

-- Query 3: Relationship with Filtering

```graphql
select users@u[*].{
  user_id: u.id,
  name,
  universities: u.studies_at_rel@sa[*].universities_node[**].{
    name,
    type
  }
}
from users:User
links
  (users) -> [studies_at_rel:UserStudiesAtUniversity] -> (universities_node:University where type = "Public");
```

-- Query 4: Multiple Independent Traversals

```graphql
select users@u[*].{
  user_id: u.id,
  name,
  universities: u.studies_at_rel@sa[*].university_node[**].{
    name,
    type
  },
  blogs: u.writes_blog_rel@w[*].blog_node[**].{
    date_written: w.published_date,
    title,
    published_date
  }
}
from users:User
links
  (users) -> [studies_at_rel:UserStudiesAtUniversity] -> (university_node:University),
  (users) -> [writes_blog_rel:UserWritesBlog] -> (blog_node:Blog);
```

-- Query 5: Combining Paths with Logical Operators

```graphql
select users@u[*].{
  user_id: u.id,
  name,
  contributions: u.contributes_content_rel@act[*].content_items@ci[*].title
}
from users:User
links
  (users) -> [contributes_content_rel: UserWritesBlog | UserReviewsBook] -> (content_items: Blog | Book);
```

-- Query 6: Complex Nested Traversals (Version 1)

```graphql
select users@u[*].{
  user_id: u.id,
  name,
  books: u.writes_book_rel@w_edge[*].book_node@b[**].{
    title: b.title,
    reviews: b.has_review_rel@rev_edge[*].review_node@rev[**].{
      reviewer_name: rev.is_reviewed_by_user_rel@rv_by_edge[*].reviewer_node@rvr[*].name,
      reactions: rev.has_reaction_rel@rt_edge[*].reaction_node@react[**].{
        type: react.type,
        reacted_by: react.is_made_by_user_rel@rb_by_edge[*].reactor_node@rctr[*].name
      }
    }
  }
}
from users:User
links
  (users) -> [writes_book_rel:UserWritesBook] -> (book_node:Book)
          -> [has_review_rel:BookHasReview] -> (review_node:Review)
          -> [has_reaction_rel:ReviewHasReaction] -> (reaction_node:Reaction),
  (reaction_node) <- [is_made_by_user_rel:UserMadeReaction] <- (reactor_node:User),
  (review_node) <- [is_reviewed_by_user_rel:UserReviewedBook] <- (reviewer_node:User);
```

-- Query 6: Complex Nested Traversals (Version 2)

```graphql
select users@u[*].{
  user_id: u.id,
  name,
  books: u.writes_book_rel@w_edge[*].book_node@b[**].{
    title: b.title,
    reviews: b.has_review_rel@rev_edge[*].review_node@rev[**].{
      reviewer_names: rev.is_reviewed_by_user_rel@rv_by_edge[*].reviewer_node@rvr[*].name,
      reaction_types: rev.has_reaction_rel@rt_edge[*].reaction_node@react[*].type,
      reacted_by_names: rev.has_reaction_rel@rt_edge[*].reaction_node@react[*].is_made_by_user_rel@rb_by_edge[*].reactor_node@rctr[*].name
    }
  }
}
from users:User
links
  (users) -> [writes_book_rel:UserWritesBook] -> (book_node:Book)
          -> [has_review_rel:BookHasReview] -> (review_node:Review)
          -> [has_reaction_rel:ReviewHasReaction] -> (reaction_node:Reaction),
  (reaction_node) <- [is_made_by_user_rel:UserMadeReaction] <- (reactor_node:User),
  (review_node) <- [is_reviewed_by_user_rel:UserReviewedBook] <- (reviewer_node:User);
```

-- Query 7: Path-Specific Filters

```graphql
select users@u[*].{
  user_id: u.id,
  name,
  universities: u.sings_at_rel@s_edge[*].university_node[**].{
    name,
    founded_date
  }
}
from users:User
links
  (users) -> [sings_at_rel:UserSingsAtUniversity] -> (university_node@un:University where un.founded_date < dt"1900-01-01");
```

-- Query 8: Global Filters

```graphql
select users@u[*].{
  user_id: u.id,
  blogs: u.writes_blog_rel@w_edge[*].blog_node@b[**].{
    title
  }
}
from users@uf:User
links
  (users) -> [writes_blog_rel:UserWritesBlog] -> (blog_node:Blog)
where uf.age > 30;
```

-- Query 9: Complex Query with Path-Specific and Global Filters

```graphql
select users@u[*].{
  user_id: u.id,
  blogs: u.writes_blog_rel@w_edge[*].blog_node@b[**].{
    title: b.title
  }
}
from users:User
links
  (users) -> [writes_blog_rel:UserWritesBlog where writes_blog_rel.published_date > "2020-01-01"] -> (blog_node:Blog where blog_node.views > 10000)
where u.age > 30;
```

-- Query 10: Reusable Query Fragments

```graphql
let user_likes_fragment = (
  select users@u[*].{
    user_id: u.id,
    liked_titles_list: u.likes_content_rel@l_edge[*].liked_content_nodes@li[*].title
  }
  from users:User
  links
    (users) -> [likes_content_rel:UserLikesContent] -> (liked_content_nodes:Content)
);

select user_likes_fragment@ul_item[*].{
  user_id: ul_item.user_id,
  likes_count: count(ul_item.liked_titles_list)
}
from user_likes_fragment;
```

-- Query : Flattened List of Nested Objects

```graphql
select users@u[*].{
  user_id: u.id,
  user_name: u.name,
  books_written_titles: u.writes_book_rel@wb_e[*].book_node@b[*].title,
  genres_curated_by: u.writes_book_rel@wb_e[*].book_node@b[*].belongs_to_genre_rel@ca_e[*].genre_node@g[*].is_curated_by_editor_rel@cb_e[*].curator_node@cur[*].name,
  blogs_written_titles: u.writes_blog_rel@wbl_e[*].blog_node@bl[*].title,
  blog_comments_texts: u.writes_blog_rel@wbl_e[*].blog_node@bl[*].has_comment_rel@hbc_e[*].comment_node@cm[*].text
}
FROM users:User
links
  (users) -> [writes_book_rel:UserWritesBook] -> (book_node:Book)
          -> [belongs_to_genre_rel:BookBelongsToGenre] -> (genre_node:Genre),
  (genre_node) <- [is_curated_by_editor_rel:EditorCuratesGenre] <- (curator_node:User),
  (users) -> [writes_blog_rel:UserWritesBlog] -> (blog_node:Blog)
          -> [has_comment_rel:BlogHasComment] -> (comment_node:Comment);
```

-- Query : Flat structure

```graphql
select users@u[*].{
  user_id: u.id,
  user_name: u.name,
  singing_places: u.sings_at_rel@sa_e[*].place,
  blog_titles: u.writes_blog_rel@w_e[*].blog_node@b[*].title
}
FROM users:User
links
  (users) -> [sings_at_rel:UserSingsAtUniversity] -> (university_nodes:University), // Assuming place is on edge
  (users) -> [writes_blog_rel:UserWritesBlog] -> (blog_node:Blog);
```

-- Query : Relationship with Filtering and reassigning

```graphql
select users@u[*].{
  user_id: u.id,
  user_name: u.name,
  singing_places: u.sings_at_rel@sa_e[*].place,
  admirers: u.is_loved_by_user_rel@ab_e[*].admirer_node@adm[**].{
    admirer_name: adm.name
  }
}
FROM users:User
links
  (users) -> [sings_at_rel:UserSingsAtUniversity] -> (university_nodes:University), // Assuming place is on edge
  (users) <- [is_loved_by_user_rel:UserLovesUser] <- (admirer_node:User);
```

-- Query : Node and Edge and or

```graphql
select users@u[*].{
  user_id: u.id,
  target_titles: u.executes_target_rel@act_e[*].target_nodes@tgt[*].title
}
FROM users:User
links
  (users) -> [executes_target_rel: UserExecutesQuery | UserExecutesTask] -> (target_nodes: Query | Task);
```

-- Query : Group By and Aggregation

```graphql
select groups@g[*].{
  country: g.country,
  state: g.state,
  city: g.city,
  total_users: count(g.users@usr[*]), // usr refers to items from the 'users' collection in this group
  universities: g.users@usr[*].studies_at_rel@sa_e[*].university_node@uni[**].{
    start_date: sa_e.date,
    name: uni.name,
    type: uni.type
  }
}
FROM users:User // users is the collection alias, u is the item alias in FROM
links
  (users@u_from_item) -> [studies_at_rel:UserStudiesAtUniversity] -> (university_node:University) // u_from_item is item from users collection
group by { country: u_from_item.country, state: u_from_item.state, city: u_from_item.city } into groups;
```

-- Query: Very Complex (First one)

```graphql
select users@u[*].{
  user_id: u.id,
  user_name: u.name,
  books_written: u.writes_book_rel@w_e[*].book_node@b[**].{
    published_date: w_e.publishedDate,
    title: b.title,
    genres: b.belongs_to_genre_rel@ca_e[*].genre_node@g[**].{
      name: g.name,
      curated_by: g.is_curated_by_editor_rel@cb_e[*].curator_node@cur[*].name
    },
    reviews: b.has_review_rel@rev_e[*].review_node@rev[**].{
      reviewer_name: rev.user_id,
      review_content: rev.content,
      reactions: rev.has_reaction_rel@rtw_e[*].reaction_node@react[**].{
        type: react.type,
        reacted_by: react.user_id
      }
    }
  },
  blogs_written: u.writes_blog_rel@wb_e[*].blog_node@bl[**].{
    title: bl.title,
    related_books: bl.is_related_to_book_rel@rl_e[*].related_book_node@bbk[*].title,
    comments: bl.has_comment_rel@co_e[*].comment_node@cm[**].{
      commenter: cm.user_id,
      comment_text: cm.text
    }
  }
}
FROM users:User
links
  (users) -> [writes_book_rel:UserWritesBook] -> (book_node:Book)
          -> [belongs_to_genre_rel:BookBelongsToGenre] -> (genre_node:Genre),
  (genre_node) <- [is_curated_by_editor_rel:EditorCuratesGenre] <- (curator_node:User),
  (book_node)  -> [has_review_rel:BookHasReview] -> (review_node:Review)
               -> [has_reaction_rel:ReviewHasReaction] -> (reaction_node:Reaction),
  (users) -> [writes_blog_rel:UserWritesBlog] -> (blog_node:Blog)
          -> [is_related_to_book_rel:BlogRelatedToBook] -> (related_book_node:Book),
  (blog_node)  -> [has_comment_rel:BlogHasComment] -> (comment_node:Comment);
```

-- Query: Very Complex (Second one, person/product)

```graphql
select persons@p[*].{
  person_id: p.id,
  purchased_products: p.purchased_product_rel1@pch1_e[*].product_node1@p1_node[**].{
    product_name: p1_node.name,
    buyers: p1_node.was_purchased_by_user_rel@pch2_e[*].buyer_node@buy_node[**].{
      buyer_id: buy_node.id,
      purchase_date: pch2_e.created_at
    }
  }
}
FROM persons:Person
links (persons) -> [purchased_product_rel1:UserPurchasedProduct] -> (product_node1:Product),
      (product_node1) <- [was_purchased_by_user_rel:UserPurchasedProduct] <- (buyer_node:Person),
      (buyer_node) -> [purchased_product_rel3:UserPurchasedProduct where created_at > time::now() - 3w] -> (product_node2:Product);
```

--- Query: Very Complex (Third one, users/blogs/books, simplified)

```graphql
select users@u[*].{
  user_id: u.id,
  blogs_written: u.writes_blog_rel@wbl_e[*].blog_node@bl[**].{
    title: bl.title,
    comments: bl.has_comment_rel@co_e[*].comment_node@cm[**].{
      commenter: cm.user_id,
      comment_text: cm.text
    }
  },
  books_written: u.writes_book_rel@wbk_e[*].book_node@bk[**].{
    title: bk.title,
    published_date: wbk_e.publishedDate,
    reviews: bk.has_review_rel@rev_e[*].review_node@rev[**].{
      reviewer_name: rev.user_id,
      review_content: rev.content
    }
  }
}
FROM users:User
links
  (users) -> [writes_book_rel:UserWritesBook] -> (book_node:Book)
          -> [has_review_rel:BookHasReview] -> (review_node:Review),
  (users) -> [writes_blog_rel:UserWritesBlog] -> (blog_node:Blog)
          -> [has_comment_rel:BlogHasComment] -> (comment_node:Comment);
```

--- Query: select Range (Corrected `Email` relationship)
Assuming:

- `PersonSendsEmail` (\_from: Person, \_to: Email)
- `EmailReceivedByPerson` (\_from: Email, \_to: Person)

```graphql
select persons@p[6:90].{
  person_id: p.id,
  person_name: p.name,
  sent_emails: p.sends_email_rel@s_e[*].email_node@e[**].{
    email_id: e.id,
    subject: e.subject,
    recipients: e.is_received_by_person_rel@to_e[*].recipient_node@r[*].name
  }
}
FROM persons:Person
links (persons) -> [sends_email_rel:PersonSendsEmail] -> (email_node:Email)
                 -> [is_received_by_person_rel:EmailReceivedByPerson] -> (recipient_node:Person)
where count(p.sends_email_rel@s_e[*]) > 5;
```

**Alternative if `PersonSendsEmail` edge itself has a `recipients` field (array of Person IDs):**
If `PersonSendsEmail` edge was `(_from: Person, _to: Email, recipients: Array<LinkOne<Person>>)`

```graphql
select persons@p[6:90].{
  person_id: p.id,
  person_name: p.name,
  sent_emails: p.sends_email_rel@s_e[*].email_node@e[**].{ // email_node is the target Email of PersonSendsEmail
    email_id: e.id, // or s_e.to (if 'to' is the email id on the edge)
    subject: e.subject, // or email_node.subject if 'e' is the email document
    // Assuming s_e (the edge instance) has a field 'recipients' which is Array<LinkOne<Person>>
    recipient_names: s_e.recipients@rec_link[*].recipient_node@r[*].name
  }
}
from persons:Person
links (persons) -> [sends_email_rel:PersonSendsEmail] -> (email_node:Email) // This links person to the email they sent
      // To get recipient details, we'd need to dereference from the edge's recipient field
      // This part of the links might not be needed if recipients are on the sends_email_rel edge.
      // If sends_email_rel.recipients is Array<RecordId<Person>>, then:
      // (sends_email_rel) -> [implicit_link_to_recipient] -> (recipient_node:Person)
where count(p.sends_email_rel@s_e[*]) > 5;
```

I'll stick to the first interpretation (two distinct edge types) for consistency for now, as it's more explicit.

--- Query: select Conditional (Corrected `Email` relationship)

```graphql
select persons@p[where p.age > 30].{
  person_id: p.id,
  person_name: p.name,
  sent_emails: p.sends_email_rel@s_e[*].email_node@e[**].{
    email_id: e.id,
    subject: e.subject,
    recipients: e.is_received_by_person_rel@to_e[*].recipient_node@r[*].name
  }
}
from persons:Person
links (persons) -> [sends_email_rel:PersonSendsEmail] -> (email_node:Email)
                 -> [is_received_by_person_rel:EmailReceivedByPerson] -> (recipient_node:Person)
where count(p.sends_email_rel@s_e[*]) > 5;
```

--- Query: select Specific (Corrected `Email` relationship)

```graphql
select persons@p[0].{
  person_id: p.id,
  person_name: p.name,
  sent_emails: p.sends_email_rel@s_e[*].email_node@e[**].{
    email_id: e.id,
    subject: e.subject,
    recipients: e.is_received_by_person_rel@to_e[*].recipient_node@r[*].name
  }
}
from persons:Person
links (persons) -> [sends_email_rel:PersonSendsEmail] -> (email_node:Email)
                 -> [is_received_by_person_rel:EmailReceivedByPerson] -> (recipient_node:Person)
where count(p.sends_email_rel@s_e[*]) > 5;
```

-- Query: Multilevel Nested Traversal (First one)

```graphql
select persons@p[*].{
  person_id: p.id,
  connections: p.knows_person_rel1@t1_e[*].friend1_node@f1_n[**].{
    level_2: f1_n.knows_person_rel2@t2_e[*].friend2_node@f2_n[**].{
      level_3: f2_n.knows_person_rel3@t3_e[*].friend3_node@f3_n[**].{
        name: f3_n.name,
        in_person: t3_e.inperson
      }
    }
  }
}
from persons:Person
links (persons) -> [knows_person_rel1:PersonKnowsPerson] -> (friend1_node:Person)
                 -> [knows_person_rel2:PersonKnowsPerson] -> (friend2_node:Person)
                 -> [knows_person_rel3:PersonKnowsPerson where inperson = true] -> (friend3_node:Person)
where p.id = "john";
```

-- Query: Multilevel Nested Traversal (Second one, broken links)

```graphql
select persons@p[*].{
  person_id: p.id,
  person_name: p.name,
  connections: p.knows_f1_rel@t1_e[*].friend1_node@f1_n[**].{
    level_2: f1_n.knows_f2_rel@t2_e[*].friend2_node@f2_n[**].{
      level_3: f2_n.knows_f3_rel@t3_e[*].friend3_node@f3_n[**].{
        name,
        in_person: t3_e.inperson
      }
    }
  }
}
from persons:Person
links (persons) -> [knows_f1_rel:PersonKnowsPerson] -> (friend1_node:Person),
      (friend1_node) -> [knows_f2_rel:PersonKnowsPerson] -> (friend2_node:Person),
      (friend2_node) -> [knows_f3_rel:PersonKnowsPerson where inperson = true] -> (friend3_node:Person)
where p.name = "john";
```

--- Query: Multilevel Nested Traversal (users/blogs/books, repeated)

```graphql
select users@u[*].{
  user_id: u.id,
  blogs_written: u.writes_blog_rel@wbl_e[*].blog_node@bl[**].{
    title: bl.title,
    comments: bl.has_comment_rel@co_e[*].comment_node@cm[**].{
      commenter: cm.user_id,
      comment_text: cm.text
    }
  },
  books_written: u.writes_book_rel@wbk_e[*].book_node@bk[**].{
    title: bk.title,
    reviews: bk.has_review_rel@rev_e[*].review_node@rev[**].{
      reviewer_name: rev.user_id,
      review_content: rev.content
    }
  }
}
from users:User
links
  (users) -> [writes_blog_rel:UserWritesBlog] -> (blog_node:Blog)
          -> [has_comment_rel:BlogHasComment] -> (comment_node:Comment),
  (users) -> [writes_book_rel:UserWritesBook] -> (book_node:Book)
          -> [has_review_rel:BookHasReview] -> (review_node:Review);
```

--- Grouping ---

```graphql
select groups@g[*].{
  tag: g.key_tags,
  projects: g.users@usr[*].has_project_rel@hp_e[*].project_nodes@p[where g.key_tags IN p.tags][*].name
}
from users:User // users is collection, u_from_item is item in from
links (users@u_from_item) -> [has_project_rel:UserHasProject] -> (project_nodes:Project)
group by { key_tags: u_from_item.has_project_rel@hp_e[*].project_nodes@p[*].tags[*] } into groups;
```

--- (Grouping queries without links remain unchanged)

```graphql
select groups@g[where max(g.weather_reports@w_grp_item[*].temp_low) < 40].{
  city: g.city,
  count: count(g.weather_reports@w_grp_item[*]),
  max_temp: max(g.weather_reports@w_grp_item[*].temp_low),
  weather_info: g.weather_reports@w_grp_item[**].{
    place,
    time
  }
}
from weather_reports:Weather // Pluralized collection alias
group by { city: weather_reports@w_src_item.city } into groups;
```

```graphql
select groups@g[order by total_users desc].{
  city: g.city,
  total_users: array::count(g.users@u_grp[*])
}
from users:User
group by { city: users@u_src.city } into groups;
```

```graphql
select groups@g[*].{
  group_name: string::concat(g.country, "-", math::round(math::mean(g.users@u_grp[*].age), 0)),
  total_users: array::count(g.users@u_grp[*])
}
from users:User
group by { country: users@u_src.country } into groups;
```

```graphql
select groups@g[where max(g.weather_reports@w_grp[*].temp_low) < 40 order by count(g.weather_reports@w_grp[*]) desc].{
  country: g.country,
  state: g.state,
  city: g.city,
  city_alias: g.city,
  state_info: {
    state: g.state,
    city: g.city,
    weather_info: g.weather_reports@w_grp[**].{
      temp_low: w_grp.temp_low,
      place: w_grp.place,
      time: w_grp.time
    }
  }
}
from weather_reports:Weather // Pluralized
group by { country: weather_reports@w_src.country, state: weather_reports@w_src.state, city: weather_reports@w_src.city } into groups;
```

**Flattening Examples (with plural collection aliases)**

**Method 1: Explicit Field Projection**

```graphql
select users@u[*].writes_book_rel@w[*].book_node@b[**].{
    user_id: u.id,
    user_name: u.name,
    user_age: u.age,
    published_date: w.published_date,
    book_id: b.id,
    book_title: b.title,
    book_genre: b.genre
}
from users:User
links
  (users) -> [writes_book_rel:UserWritesBook] -> (book_node:Book);
```

**Method 2: Spread Operator (`...`)**

```graphql
select users@u[*].writes_book_rel@w[*].book_node@b[*].{
    ...u,
    ...w,
    ...b,
    // Explicit aliasing for potential collisions:
    edge_id: w.id, // if 'id' is a field on the edge and might collide
    book_id_explicit: b.id
}
from users:User
links
  (users) -> [writes_book_rel:UserWritesBook] -> (book_node:Book);
```

**Addressing the `complex_query.sql` and `complex2.sql` examples from your context:**

-- These queries showcase many advanced features: `LET` variable bindings, subqueries in projections, complex filtering within array accessors, and multiple `links` paths. The principles applied above (plural collection aliases, item aliases for iteration, consistent EdgeType naming, correct traversal arrows) would be applied to them as well.
--
-- Example snippet from `complex2.sql` refactored:
-- Original:
--
-- ```
-- from (user:User where user.age > 50)
-- links
--   (user where user.name == 'oye') -> [writes:UserWritesBlog where writes.published_date > date_2024] -> (blog:Blog where blog.views > writes.score and blog.name == user.fav_blog_name) <- [read_by : UserReadsBlog] <- (young_user: User where $this.age < 20 and $this.age > user.age),
-- (user where user.friend_count > 3) -> [performs: UserPerformsAtEvent where $this.attendance > writes.count or writes.count > 77] ,
-- (horse@h: Animal where h.name == 'horse') -> [carries: AnimalCarriesUser where $this.year > dt'2024'] -> (user),
-- (user)<-[carried_after_2023_by: AnimalCarriesUser where $this.year > dt'2023'] <- (donkey: Animal where $this.name == 'donkey')
```

Refactored (focusing on aliases and EdgeTypes):

```graphql
from (users_coll@usr:User where usr.age > 50) // users_coll is collection, usr is item
links
  (users_coll@u_writes where u_writes.name == 'oye') // u_writes is an item from users_coll
    -> [writes_blog_rel:UserWritesBlog where writes_blog_rel.published_date > date_2024]
    -> (blog_nodes@b_written where b_written.views > writes_blog_rel.score AND b_written.name == u_writes.fav_blog_name) // b_written is item from blog_nodes
    <- [is_read_by_rel:UserReadsBlog] // Assuming UserReadsBlog is _from: User, _to: Blog
    <- (young_users_coll@yu where yu.age < 20 AND yu.age > u_writes.age), // young_users_coll is collection, yu is item

  (users_coll@u_performs where u_performs.friend_count > 3) // u_performs is an item from users_coll
    -> [performs_at_event_rel:UserPerformsAtEvent where performs_at_event_rel.attendance > writes_blog_rel.count OR writes_blog_rel.count > 77],
    // Note: writes_blog_rel.count might be out of scope here if this LINK path is independent.
    // If it depends on the first path, the structure needs to reflect that chaining.

  (animal_coll_horse@h where h.name == 'horse') // animal_coll_horse is collection, h is item
    -> [carries_user_rel:AnimalCarriesUser where carries_user_rel.year > dt'2024']
    -> (users_coll@u_carried), // u_carried is an item from users_coll (the one being carried)

  (users_coll@u_is_carried) // u_is_carried is an item from users_coll
    <- [is_carried_by_animal_rel:AnimalCarriesUser where is_carried_by_animal_rel.year > dt'2023']
    <- (animal_coll_donkey@d where d.name == 'donkey') // animal_coll_donkey is collection, d is item
```
