import type { AttemptRecord, EventView, RequestRecord } from "../api/types";
import { duration, istClock, istDate } from "../format";

function when(unix: number): string {
  return `${istDate(unix).slice(5)} ${istClock(unix)}`;
}

export function EventLog({ events }: { events: EventView[] }) {
  return (
    <section className="panel" aria-label="activity">
      <h3>Activity</h3>
      {events.length === 0 ? (
        <p className="muted">Nothing has happened yet.</p>
      ) : (
        <ul className="log">
          {events.slice(0, 40).map((event, index) => (
            <li key={`${event.at}-${index}`} className={`log-row kind-${event.kind}`}>
              <time className="mono">{istClock(event.at)}</time>
              <span className="log-kind">{event.kind}</span>
              <span>
                {event.alias === null ? "" : <strong>{event.alias} </strong>}
                {event.message}
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

export function RequestLog({ requests }: { requests: RequestRecord[] }) {
  return (
    <section className="panel" aria-label="token requests">
      <h3>Token API requests</h3>
      {requests.length === 0 ? (
        <p className="muted">No engine has asked for a token yet.</p>
      ) : (
        <table className="table">
          <thead>
            <tr>
              <th>when (IST)</th>
              <th>account</th>
              <th>from</th>
              <th>outcome</th>
              <th>reason</th>
            </tr>
          </thead>
          <tbody>
            {requests.map((request, index) => (
              <tr key={`${request.at}-${index}`}>
                <td className="mono">{when(request.at)}</td>
                <td>{request.alias ?? "unknown"}</td>
                <td className="mono">{request.ip ?? "?"}</td>
                <td className={request.outcome === "served" ? "tone-done" : "tone-failed"}>{request.outcome}</td>
                <td>{request.reason}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

export function AttemptLog({ attempts }: { attempts: AttemptRecord[] }) {
  return (
    <section className="panel" aria-label="login attempts">
      <h3>Login attempts</h3>
      {attempts.length === 0 ? (
        <p className="muted">No login has run yet.</p>
      ) : (
        <table className="table">
          <thead>
            <tr>
              <th>when (IST)</th>
              <th>account</th>
              <th>path</th>
              <th>took</th>
              <th>outcome</th>
              <th>detail</th>
            </tr>
          </thead>
          <tbody>
            {attempts.map((attempt, index) => (
              <tr key={`${attempt.started_at}-${index}`}>
                <td className="mono">{when(attempt.started_at)}</td>
                <td>{attempt.alias}</td>
                <td>{attempt.drive}</td>
                <td className="mono">{duration(attempt.finished_at - attempt.started_at)}</td>
                <td className={attempt.outcome === "ok" ? "tone-done" : "tone-failed"}>
                  {attempt.outcome === "ok" ? "ok" : `failed at ${attempt.stage}`}
                </td>
                <td>{attempt.detail}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
