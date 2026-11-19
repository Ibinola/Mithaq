import type { Response } from "express";

const ERROR_CODES: Record<number, string> = {
  400: "INVALID_REQUEST",
  401: "UNAUTHORIZED",
  403: "FORBIDDEN",
  404: "NOT_FOUND",
  413: "PAYLOAD_TOO_LARGE",
  422: "VALIDATION_FAILED",
  429: "RATE_LIMITED",
  500: "INTERNAL_ERROR",
  502: "UPSTREAM_ERROR",
  503: "SERVICE_UNAVAILABLE",
};

export function sendApiError(
  res: Response,
  status: number,
  message: string,
  details: Record<string, unknown> = {},
): void {
  res.status(status).json({
    error: message,
    code: ERROR_CODES[status] ?? "REQUEST_FAILED",
    requestId: res.locals["requestId"],
    ...details,
  });
}
