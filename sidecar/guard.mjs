const DEFAULT_CONSENT_HOSTS = ["auth.dhan.co"];

export function consentHosts(value) {
  const hosts = String(value || "")
    .split(",")
    .map((host) => host.trim().toLowerCase())
    .filter((host) => host !== "");
  return hosts.length > 0 ? hosts : DEFAULT_CONSENT_HOSTS;
}

export function refusal(request, hosts) {
  let consent;
  let redirect;
  try {
    consent = new URL(request.consentUrl);
    redirect = new URL(request.redirectUrl);
  } catch {
    return "consentUrl and redirectUrl must be URLs";
  }
  if (consent.protocol !== "https:" || !hosts.includes(consent.hostname.toLowerCase())) {
    return `consentUrl must be https on ${hosts.join(", ")}`;
  }
  if (!["http:", "https:"].includes(redirect.protocol)) {
    return "redirectUrl must be http or https";
  }
  if (!/^[A-Za-z0-9]{4,32}$/.test(request.pin)) {
    return "pin is malformed";
  }
  if (!/^[0-9]{6}$/.test(request.totp)) {
    return "totp must be six digits";
  }
  return null;
}
