export class AccessError extends Error {
  constructor(
    message: string,
    public status: number,
  ) {
    super(message);
  }
}
export async function api<T>(path: string, data?: unknown): Promise<T> {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 12000);
  try {
    const response = await fetch(path, {
      method: data === undefined ? "GET" : "POST",
      credentials: "same-origin",
      cache: "no-store",
      signal: controller.signal,
      headers:
        data === undefined ? undefined : { "Content-Type": "application/json" },
      body: data === undefined ? undefined : JSON.stringify(data),
    });
    const body = await response.json().catch(() => ({}));
    if (!response.ok)
      throw new AccessError(
        typeof body.message === "string" ? body.message : "连接暂时不可用",
        response.status,
      );
    return body as T;
  } catch (error) {
    if (error instanceof AccessError) throw error;
    throw new AccessError(
      "无法连接电脑，请确认手机访问已开启，并连接同一局域网",
      0,
    );
  } finally {
    clearTimeout(timeout);
  }
}
