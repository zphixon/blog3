drop table if exists post;
drop table if exists old;
drop table if exists slug;

create table post (
    id blob unique not null primary key,
    title text not null,
    subtitle text,
    published datetime not null,
    content text not null
);

create table old (
    id blob not null,
    data text,
    foreign key (id) references post (id)
);

create table slug (
    slug text unique not null primary key,
    id blob not null,
    newslug text,
    foreign key (id) references post (id),
    foreign key (newslug) references slug (slug)
);
