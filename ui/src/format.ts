const IST_OFFSET_SECONDS = 19_800;

export function duration(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const rest = total % 60;
  if (hours > 0) {
    return `${hours}h ${minutes}m`;
  }
  if (minutes > 0) {
    return `${minutes}m ${rest}s`;
  }
  return `${rest}s`;
}

export function istClock(unix: number): string {
  const into = (((unix + IST_OFFSET_SECONDS) % 86_400) + 86_400) % 86_400;
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(Math.floor(into / 3600))}:${pad(Math.floor((into % 3600) / 60))}:${pad(into % 60)}`;
}

export function istDate(unix: number): string {
  const shifted = new Date((unix + IST_OFFSET_SECONDS) * 1000);
  return shifted.toISOString().slice(0, 10);
}

export function ago(unix: number, now: number): string {
  const seconds = now - unix;
  return seconds < 5 ? "just now" : `${duration(seconds)} ago`;
}
