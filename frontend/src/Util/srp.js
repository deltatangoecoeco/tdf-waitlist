// "50M" for 50,000,000; "55.5M" for 55,500,000. Smaller amounts are shown in full.
export function formatIsk(amount) {
  if (amount === null || amount === undefined) return "";
  if (amount >= 1000000) return `${Number((amount / 1000000).toFixed(1))}M`;
  return amount.toLocaleString();
}

// Local wall-clock time without seconds, e.g. "14:41"
export function formatClock(dateObj) {
  return dateObj.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

// Whole calendar days from now to dateObj in local time: 0 = today, 1 = tomorrow
export function daysFromToday(dateObj) {
  const startOfDay = (d) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  return Math.round((startOfDay(dateObj) - startOfDay(new Date())) / 86400000);
}

// Clock time in UTC (EVE time) without seconds, e.g. "14:05"
export function formatClockUtc(dateObj) {
  return dateObj.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", timeZone: "UTC" });
}

const plural = (n, unit) => `${n} ${unit}${n === 1 ? "" : "s"}`;

// A length of time in words: "25 minutes", "3 hours", "2 days"
export function formatSpan(seconds) {
  const minutes = Math.max(1, Math.floor(seconds / 60));
  if (minutes < 60) return plural(minutes, "minute");
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return plural(hours, "hour");
  return plural(Math.floor(hours / 24), "day");
}

// How long ago a unix timestamp was, in short form: "just now", "3 min ago", "2 h ago", "3 d ago"
export function formatAgo(unixSeconds, nowMs = Date.now()) {
  const seconds = Math.max(0, Math.floor(nowMs / 1000 - unixSeconds));
  const minutes = Math.floor(seconds / 60);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours} h ago`;
  return `${Math.floor(hours / 24)} d ago`;
}
