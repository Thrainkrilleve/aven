import { Env, PackageRecord, SharedModelRecord } from "./types";

export default {
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
    "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type, Authorization",
  };

  if (method === "OPTIONS") {
    return new Response(null, { headers: jsonHeaders });
  }

  // GET /api/v1/stats
  if (pathname === "/api/v1/stats" && method === "GET") {
    try {
      const pkgCount = await env.DB.prepare("SELECT COUNT(*) as count FROM packages").first<{ count: number }>();
      const modelCount = await env.DB.prepare("SELECT COUNT(*) as count FROM shared_models").first<{ count: number }>();
      const benchStats = await env.DB.prepare(
        "SELECT COUNT(*) as solves, AVG(solve_time_us) as avg_latency FROM telemetry_benchmarks"
      ).first<{ solves: number; avg_latency: number }>();

      return new Response(
        JSON.stringify({
          version: env.AVEN_VERSION,
          packages_indexed: pkgCount?.count ?? 0,
          models_shared: modelCount?.count ?? 0,
          telemetry_solves_logged: benchStats?.solves ?? 0,
          telemetry_mean_solve_us: benchStats?.avg_latency ?? null,
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
    const limit = Math.min(parseInt(url.searchParams.get("limit") || "50"), 100);

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

  // POST /api/v1/models/share
  if (pathname === "/api/v1/models/share" && method === "POST") {
    try {
      const { title, source_code, category } = await request.json<any>();
      if (!source_code) {
        return new Response(JSON.stringify({ error: "Missing source_code" }), { status: 400, headers: jsonHeaders });
      }

      // Compute SHA-256 in pure Web Crypto API
      const enc = new TextEncoder().encode(source_code);
      const hashBuf = await crypto.subtle.digest("SHA-256", enc);
      const hashArray = Array.from(new Uint8Array(hashBuf));
      const sha256 = hashArray.map(b => b.toString(16).padStart(2, "0")).join("");

      // Alphanumeric short ID (e.g., first 8 chars of hash or random nano)
      const id = sha256.slice(0, 8);

      await env.DB.prepare(
        `INSERT INTO shared_models (id, title, category, source_code, sha256)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET views = views + 1`
      ).bind(id, title || "Untitled Model", category || "general", source_code, sha256).run();

      const url = new URL(request.url);
      const shortUrl = `${url.origin}/p/${id}`;

      return new Response(JSON.stringify({ id, shortUrl, sha256 }), { status: 201, headers: jsonHeaders });
    } catch (err: any) {
      return new Response(JSON.stringify({ error: err.message }), { status: 400, headers: jsonHeaders });
    }
  }

  // POST /api/v1/telemetry
  if (pathname === "/api/v1/telemetry" && method === "POST") {
    try {
      const { model_name, solve_time_us, facts_settled, platform } = await request.json<any>();
      if (!model_name || solve_time_us === undefined) {
        return new Response(JSON.stringify({ error: "Missing telemetry params" }), { status: 400, headers: jsonHeaders });
      }

      await env.DB.prepare(
        "INSERT INTO telemetry_benchmarks (model_name, solve_time_us, facts_settled, platform) VALUES (?, ?, ?, ?)"
      ).bind(model_name, solve_time_us, facts_settled || 0, platform || "web-wasm").run();

      return new Response(JSON.stringify({ recorded: true }), { status: 201, headers: jsonHeaders });
    } catch (err: any) {
      return new Response(JSON.stringify({ error: err.message }), { status: 500, headers: jsonHeaders });
    }
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
