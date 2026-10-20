export type View =
  | { kind: "recent" }
  | { kind: "category"; category: string }
  | { kind: "settings" };
