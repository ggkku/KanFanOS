/** Tauri 命令返回的错误是字符串，其他异常是 Error 对象。 */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
