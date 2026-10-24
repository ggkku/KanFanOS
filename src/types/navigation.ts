export type View =
  | { kind: "recent" }
  | { kind: "resources" }
  | { kind: "category"; category: string }
  | { kind: "settings" };
