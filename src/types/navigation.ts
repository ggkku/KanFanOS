export type View =
  | { kind: "recent" }
  | { kind: "rss" }
  | { kind: "resources" }
  | { kind: "category"; category: string }
  | { kind: "settings" };
