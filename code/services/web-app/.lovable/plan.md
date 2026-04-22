

## Goal
Port the `kong-ericcson/frontend` web-app (currently React Router v7) into this Lovable project (TanStack Start), then connect to GitHub for bidirectional sync so you can keep developing in either place.

## Important caveat (read first)
Lovable **cannot import an existing GitHub repo directly**. The flow is:
1. We rebuild the frontend code inside this Lovable project first.
2. Then you connect GitHub via **Connectors → GitHub → Connect project**, which creates a **new repo** under your account (e.g. `frontend-lovable`) and turns on bidirectional sync.
3. The original `kong-ericcson/frontend` repo stays as-is. If you want them merged later, you can manually copy files between the two.

The Rust backend (`bluetext-model-*`, `services/api`, `services/kong`, `code/clients`, `code/models`) stays in the original repo — Lovable's serverless Worker runtime cannot run Rust services. We'll keep the frontend pointing at your existing API via an env var.

## What gets ported (from `code/services/web-app/`)

**Frameworks → mapping**
- React Router v7 framework mode → **TanStack Start** (already in place)
- `app/routes.ts` + `app/routes/home.tsx` → `src/routes/index.tsx`
- `app/root.tsx` → `src/routes/__root.tsx`
- `app/app.css` → `src/styles.css`
- Path alias `~/*` → `@/*` (already configured)

**Files copied 1:1** (only the import paths need adjusting from `~/` to `@/`)
- All 46 shadcn `app/components/ui/*.tsx` → `src/components/ui/*` (already present in identical form — we'll diff and only overwrite if the upstream version differs)
- `app/hooks/use-mobile.ts` → `src/hooks/use-mobile.tsx` (already present)
- `app/lib/utils.ts` → `src/lib/utils.ts` (already present)

**Files ported with adjustments**
- `app/root.tsx` → `src/routes/__root.tsx`: keep the Inter font preconnect + stylesheet links, the system-preference dark-mode script, and the html/body shell. Replace React Router's `Meta`/`Links`/`Scripts`/`ScrollRestoration` with TanStack's `HeadContent`/`Scripts` (scroll restoration is already enabled in `getRouter`).
- `app/routes/home.tsx` → `src/routes/index.tsx`: same Card UI, "hello from web-app" + API status badge. The `meta()` function becomes a `head()` in `createFileRoute`. Replace `useEffect`+`fetch("/api/hello")` with a TanStack Query `useQuery` for cleaner loading/error states (optional — can keep useEffect if you prefer). API base URL pulled from `import.meta.env.VITE_API_URL` (with `/api` fallback).
- `app/app.css` → merge the upstream tokens into `src/styles.css`. Upstream uses identical `@theme inline` shape, plus an Inter `--font-sans` variable and a `@media (prefers-color-scheme: dark)` body block. The oklch color tokens from upstream replace the current placeholder values.
- `ErrorBoundary` → already covered by `DefaultErrorComponent` in `src/router.tsx` and `NotFoundComponent` in `__root.tsx`.

**Dependencies to add** (matching upstream `package.json`)
All Radix primitives the upstream uses, plus: `@hookform/resolvers`, `cmdk`, `date-fns`, `embla-carousel-react`, `input-otp`, `lucide-react`, `next-themes`, `react-day-picker`, `react-hook-form`, `react-resizable-panels`, `recharts`, `sonner`, `tailwind-merge`, `vaul`, `zod`, `tw-animate-css`, `class-variance-authority`, `clsx`. Most are already present from the shadcn baseline — installer will only add the missing ones.

**Not ported** (intentionally)
- `react-router`, `@react-router/*`, `react-router.config.ts`, `vite-tsconfig-paths`, `@tailwindcss/vite` — TanStack Start uses its own equivalents already wired up.
- Rust/Cargo/CI workflow files — backend services live elsewhere.
- `bun.lock`, `tsconfig.json`, `vite.config.ts` — Lovable's versions stay.

## API connection
The home page calls `/api/hello`. Two options for hooking it back up to your Rust API:
- **Same-origin (recommended once deployed alongside Kong):** keep `/api/hello` and rely on a reverse proxy.
- **Cross-origin (works immediately for dev):** add a `VITE_API_URL` env var, fetch `${import.meta.env.VITE_API_URL}/hello`. We'll use this pattern with a sensible default so it works out of the box.

## After implementation — connecting GitHub for bidirectional sync
You'll do this yourself in the UI (one-time, ~30 seconds):
1. Open **Connectors** in the sidebar → **GitHub** → **Connect project**.
2. Authorize the Lovable GitHub App on the account/org you want.
3. Click **Create Repository**. A new repo (e.g. `frontend-lovable`) is created and seeded with this project's code.
4. From then on, every edit in Lovable pushes to GitHub, and every push to that repo syncs back to Lovable.

If you want this code to ultimately live back inside `kong-ericcson/frontend` under `code/services/web-app/`, the practical path is: let Lovable own a standalone `frontend-lovable` repo, and periodically merge changes into the monorepo's web-app folder via a manual sync (Lovable can't push into a subdirectory of an existing monorepo).

## Deliverables after the implementation step
- Working home page identical to the upstream "hello from web-app" card with API status badge.
- All 46 shadcn components available under `@/components/ui/*`.
- Inter font loaded, system dark-mode auto-detection working.
- `VITE_API_URL` env var wired up and documented.
- Project ready for the GitHub connection step.

