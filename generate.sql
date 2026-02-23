create table if not exists post (
    id blob unique not null primary key,
    title text not null,
    subtitle text,
    published datetime not null,
    content text not null,
    draft boolean not null default false
);

create table if not exists old (
    id blob not null,
    data text,
    foreign key (id) references post (id)
);

create table if not exists slug (
    slug text unique not null primary key,
    id blob not null,
    newslug text,
    foreign key (id) references post (id),
    foreign key (newslug) references slug (slug)
);
