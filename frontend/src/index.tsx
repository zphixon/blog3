import { createRoot } from "react-dom/client";
import * as types from "./bindings/index";

interface RecentCardProps {
  pageRoot: string;
  post: types.Recent;
}
function RecentCard({ pageRoot, post }: RecentCardProps) {
  let publishedDate = new Date(post.published);
  let published = new Intl.DateTimeFormat(undefined, {
    dateStyle: "long",
    timeStyle: "short",
  }).format(publishedDate);

  return (
    <>
      <p key={post.slug} className="recentPost">
        <a className="link" href={pageRoot + "/" + post.slug}>{post.title}</a>
        {post.subtitle ? <span className="subtitle">({post.subtitle})</span> : ""}
        <span className="published">posted at {published}</span>
      </p>
    </>
  );
}

interface IndexProps {
  pageRoot: string;
  posts: types.Recent[];
}
function Index({ pageRoot, posts }: IndexProps) {
  return (
    <>
      {posts.map((post) => (
        <RecentCard key={post.slug} pageRoot={pageRoot} post={post} />
      ))}
    </>
  );
}

let posts: types.Recent[] = JSON.parse(document.getElementById("posts")!.innerText);
let pageRoot = document.getElementById("pageRoot")!.innerText;

let root = document.getElementById("reactRoot")!;
createRoot(root).render(<Index pageRoot={pageRoot} posts={posts} />);
