import { clsx, type ClassValue } from 'clsx'
import { twMerge } from 'tailwind-merge'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

export function errorText(cause: unknown): string {
  if (cause instanceof Error && cause.message) return cause.message
  return String(cause)
}
