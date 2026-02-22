import { createRoot } from "react-dom/client";
import * as types from "./bindings/index";

interface PostProps {
  post: types.Post;
}
function Post({ post }: PostProps) {
  return <>
    <h1>{post.title}</h1>
    {post.content}
  </>;
}

let post: types.Post = JSON.parse(document.getElementById("post")!.innerText);
let root = document.getElementById("reactRoot")!;
createRoot(root).render(<Post post={post} />);
