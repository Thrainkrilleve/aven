import { Env, PackageRecord, SharedModelRecord } from "./types";
import { runCycle, TASKS_BY_CRON } from "./weather/cycle";
import { handleWeatherApi } from "./weather/api";

export default {
  // Cron triggers (wrangler.jsonc): one task of the live weather nowcast behind /weather
  async scheduled(controller: ScheduledController, env: Env, ctx: ExecutionContext): Promise<void> {
    const task = TASKS_BY_CRON[controller.cron];
    if (!task) {
      console.error(`no weather task for cron "${controller.cron}"`);
      return;
    }
    ctx.waitUntil(
      runCycle(env, task)
        .then((r) => console.log(`weather (${controller.cron}): ${r.runs} site(s), ${r.verified} verified, ${r.requests} requests`))
        .catch((e) => console.error(`weather (${controller.cron}) failed:`, e))
    );
  },

  // Files in public/ are served directly by the static-assets layer, so this worker only
  // runs for paths with no matching file: the API, share redirects, and the 404 page.
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const pathname = url.pathname;

    // Shortlink redirects for shared models (/p/:id -> /?model=:id)
    if (pathname.startsWith("/p/") && pathname.length > 3) {
      const modelId = pathname.slice(3).trim();
      return Response.redirect(`${url.origin}/?model=${encodeURIComponent(modelId)}#playground`, 302);
    }

    if (pathname.startsWith("/api/v1/")) {
      // HEAD is answered like GET (same status and headers) without the body
      if (request.method === "HEAD") {
        const res = await handleApi(new Request(request, { method: "GET" }), env, pathname);
        return new Response(null, { status: res.status, headers: res.headers });
      }
      return handleApi(request, env, pathname);
    }

    return env.ASSETS.fetch(request);
  },
};

// --- API Router ---
async function handleApi(request: Request, env: Env, pathname: string): Promise<Response> {
  const method = request.method.toUpperCase();
  const jsonHeaders = {
    "Content-Type": "application/json",
    "Access-Control-Allow-Origin": "*",
    "Access-Control-Allow-Methods": "GET, HEAD, POST, OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type, Authorization",
  };

  if (method === "OPTIONS") {
    return new Response(null, { headers: jsonHeaders });
  }

  if (pathname.startsWith("/api/v1/weather/") && method === "GET") {
    try {
      const res = await handleWeatherApi(env, new URL(request.url), jsonHeaders);
      if (res) return res;
    } catch (err: any) {
      return new Response(JSON.stringify({ error: err.message }), { status: 500, headers: jsonHeaders });
    }
  }

  // GET /api/v1/stats
  if (pathname === "/api/v1/stats" && method === "GET") {
    try {
      const pkgCount = await env.DB.prepare("SELECT COUNT(*) as count FROM packages").first<{ count: number }>();
      const modelCount = await env.DB.prepare("SELECT COUNT(*) as count FROM shared_models").first<{ count: number }>();
      return new Response(
        JSON.stringify({
          version: env.AVEN_VERSION,
          packages_indexed: pkgCount?.count ?? 0,
          models_shared: modelCount?.count ?? 0,
          status: "healthy",
        }),
        { headers: jsonHeaders }
      );
    } catch (err: any) {
      return new Response(JSON.stringify({ error: err.message }), { status: 500, headers: jsonHeaders });
    }
  }

  // GET /api/v1/packages
  if (pathname === "/api/v1/packages" && method === "GET") {
    const url = new URL(request.url);
    const search = url.searchParams.get("q") || "";
    const requested = parseInt(url.searchParams.get("limit") || "50", 10);
    const limit = Number.isFinite(requested) ? Math.min(Math.max(requested, 1), 100) : 50;

    try {
      let query = "SELECT * FROM packages";
      let params: any[] = [];
      if (search) {
        query += " WHERE name LIKE ? OR description LIKE ?";
        params.push(`%${search}%`, `%${search}%`);
      }
      query += " ORDER BY downloads DESC LIMIT ?";
      params.push(limit);

      const { results } = await env.DB.prepare(query).bind(...params).all<PackageRecord>();
      return new Response(JSON.stringify({ packages: results }), { headers: jsonHeaders });
    } catch (err: any) {
      return new Response(JSON.stringify({ error: err.message }), { status: 500, headers: jsonHeaders });
    }
  }

  // GET /api/v1/packages/:name
  if (pathname.startsWith("/api/v1/packages/") && method === "GET") {
    const pkgName = pathname.slice("/api/v1/packages/".length).trim();
    try {
      const { results } = await env.DB.prepare(
        "SELECT * FROM packages WHERE name = ? ORDER BY created_at DESC"
      ).bind(pkgName).all<PackageRecord>();

      if (!results || results.length === 0) {
        return new Response(JSON.stringify({ error: `Package '${pkgName}' not found` }), { status: 404, headers: jsonHeaders });
      }

      return new Response(JSON.stringify({ name: pkgName, versions: results }), { headers: jsonHeaders });
    } catch (err: any) {
      return new Response(JSON.stringify({ error: err.message }), { status: 500, headers: jsonHeaders });
    }
  }

  // POST /api/v1/packages (Publish package)
  if (pathname === "/api/v1/packages" && method === "POST") {
    // Publishing writes to the public index, so it needs the REGISTRY_PUBLISH_TOKEN secret
    // (set with `wrangler secret put REGISTRY_PUBLISH_TOKEN`). Without it, publishing is off.
    if (!env.REGISTRY_PUBLISH_TOKEN) {
      return new Response(JSON.stringify({ error: "Publishing is not enabled on this registry" }), {
        status: 503,
        headers: jsonHeaders,
      });
    }
    if (!(await tokenMatches(request.headers.get("Authorization"), env.REGISTRY_PUBLISH_TOKEN))) {
      return new Response(JSON.stringify({ error: "Unauthorized" }), { status: 401, headers: jsonHeaders });
    }
    try {
      const body = await request.json<any>();
      const { name, version, description, authors, license, entry, sha256_seal, verified_units, tarball_size } = body;

      if (!name || !version || !sha256_seal) {
        return new Response(JSON.stringify({ error: "Missing required fields: name, version, sha256_seal" }), {
          status: 400,
          headers: jsonHeaders,
        });
      }

      await env.DB.prepare(
        `INSERT INTO packages (name, version, description, authors, license, entry, sha256_seal, verified_units, tarball_size_bytes)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`
      ).bind(
        name,
        version,
        description || "",
        JSON.stringify(authors || []),
        license || "MIT",
        entry || "lib.aven",
        sha256_seal,
        JSON.stringify(verified_units || []),
        tarball_size || 0
      ).run();

      return new Response(JSON.stringify({ success: true, name, version, sha256_seal }), {
        status: 201,
        headers: jsonHeaders,
      });
    } catch (err: any) {
      return new Response(JSON.stringify({ error: err.message }), { status: 400, headers: jsonHeaders });
    }
  }

  // GET /api/v1/models/:id
  if (pathname.startsWith("/api/v1/models/") && method === "GET") {
    const id = pathname.slice("/api/v1/models/".length).trim();
    try {
      const model = await env.DB.prepare(
        "SELECT * FROM shared_models WHERE id = ?"
      ).bind(id).first<SharedModelRecord>();

      if (!model) {
        return new Response(JSON.stringify({ error: "Model not found" }), { status: 404, headers: jsonHeaders });
      }

      // Increment view counter asynchronously
      await env.DB.prepare("UPDATE shared_models SET views = views + 1 WHERE id = ?").bind(id).run();

      return new Response(JSON.stringify(model), { headers: jsonHeaders });
    } catch (err: any) {
      return new Response(JSON.stringify({ error: err.message }), { status: 500, headers: jsonHeaders });
    }
  }

  // Anonymous writes were removed: model sharing now encodes state in the page URL, and
  // telemetry rows could be inflated by anyone. Old clients get an explicit answer.
  if ((pathname === "/api/v1/models/share" || pathname === "/api/v1/telemetry") && method === "POST") {
    return new Response(JSON.stringify({ error: "This endpoint has been removed" }), {
      status: 410,
      headers: jsonHeaders,
    });
  }

  return new Response(JSON.stringify({ error: `Not found: ${pathname}` }), { status: 404, headers: jsonHeaders });
}

// Constant-time comparison of a "Bearer <token>" header against the configured secret.
async function tokenMatches(header: string | null, secret: string): Promise<boolean> {
  const presented = header?.startsWith("Bearer ") ? header.slice("Bearer ".length) : "";
  const enc = new TextEncoder();
  const [a, b] = await Promise.all([
    crypto.subtle.digest("SHA-256", enc.encode(presented)),
    crypto.subtle.digest("SHA-256", enc.encode(secret)),
  ]);
  return crypto.subtle.timingSafeEqual(a, b);
}
