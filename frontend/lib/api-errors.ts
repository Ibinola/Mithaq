import { NextResponse } from "next/server";

const ERROR_CODES: Record<number, string> = {
  400: "INVALID_REQUEST",
  401: "UNAUTHORIZED",
  403: "FORBIDDEN",
  404: "NOT_FOUND",
  409: "CONFLICT",
  413: "PAYLOAD_TOO_LARGE",
  422: "ISSUANCE_FAILED",
  429: "RATE_LIMITED",
  500: "INTERNAL_ERROR",
  502: "UPSTREAM_ERROR",
  503: "SERVICE_UNAVAILABLE",
};

/** Adds the common error fields and request correlation header to API errors. */
export async function standardizeApiErrorResponse(
  response: NextResponse,
  requestId: string,
): Promise<NextResponse> {
  response.headers.set("x-request-id", requestId);
  if (response.status < 400) return response;

  const body: unknown = await response.clone().json();
  if (!body || typeof body !== "object" || Array.isArray(body)) {
    throw new TypeError("API error responses must have a JSON object body");
  }

  const payload = body as Record<string, unknown>;
  if (typeof payload["error"] !== "string") {
    throw new TypeError("API error responses must include an error message");
  }

  const headers = new Headers(response.headers);
  headers.delete("content-length");
  headers.delete("content-encoding");
  return NextResponse.json(
    {
      ...payload,
      code:
        typeof payload["code"] === "string"
          ? payload["code"]
          : ERROR_CODES[response.status] ?? "REQUEST_FAILED",
      requestId,
    },
    { status: response.status, headers },
  );
}
