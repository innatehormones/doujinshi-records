//! Unified HTTP client for the local API.
//!
//! Every call goes through `apiGet` / `apiPost` so the bearer token
//! is injected in one place. The frontend never builds raw `fetch`
//! requests against `127.0.0.1:<port>`.
//!
//! NB: `/api/covers/*` is auth-exempt on the backend so `<img>` tags
//! can keep using the bare URL — this client is for the JSON API only.

import { useSettingsStore } from "@/stores"
import type { ConflictCompare, DetailImagesResponse, MetadataPatch } from "@/types/api"

export class ApiError extends Error {
  status: number
  body: string
  constructor(status: number, body: string) {
    super(`HTTP ${status}: ${body}`)
    this.status = status
    this.body = body
  }
}

async function authHeader(): Promise<Record<string, string>> {
  const settings = useSettingsStore()
  if (!settings.data) {
    await settings.load()
  }
  const token = settings.data?.auth_token ?? ""
  return { Authorization: `Bearer ${token}` }
}

async function ensureBase(): Promise<string> {
  const settings = useSettingsStore()
  if (!settings.data) {
    await settings.load()
  }
  return settings.apiBase
}

export async function apiGet<T>(path: string): Promise<T> {
  const base = await ensureBase()
  const headers = await authHeader()
  const resp = await fetch(base + path, { headers })
  if (!resp.ok) {
    throw new ApiError(resp.status, await resp.text())
  }
  return (await resp.json()) as T
}

export async function apiPost(path: string): Promise<Response> {
  const base = await ensureBase()
  const headers = await authHeader()
  return fetch(base + path, { method: "POST", headers })
}

export async function apiPatch(path: string, body: unknown): Promise<Response> {
  const base = await ensureBase()
  const headers = {
    ...(await authHeader()),
    "Content-Type": "application/json",
  }
  return fetch(base + path, {
    method: "PATCH",
    headers,
    body: JSON.stringify(body),
  })
}

export async function fetchCompare(conflictId: number): Promise<ConflictCompare> {
  return apiGet<ConflictCompare>(`/api/conflicts/${conflictId}/compare`)
}

export async function fetchDetailImages(id: number): Promise<DetailImagesResponse> {
  return apiGet<DetailImagesResponse>(`/api/doujinshi/${id}/images`)
}

export async function patchMetadata(id: number, patch: MetadataPatch): Promise<Response> {
  return apiPatch(`/api/doujinshi/${id}`, patch)
}