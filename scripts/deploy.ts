/**
 * deploy.ts — build locally and publish the built `_site/` to the `gh-pages`
 * branch, which GitHub Pages serves statically (a `.nojekyll` marker stops GH
 * from re-running Jekyll over the already-rendered output).
 *
 *   deno task deploy
 *
 * The build runs on local hardware; GitHub is only a static host. `_site/` is
 * pushed as a single fresh commit (force) so the `gh-pages` branch never
 * accumulates history. `master` stays source-only.
 *
 * One-time setup: GitHub repo Settings → Pages → Source → "Deploy from a
 * branch" → `gh-pages` / `(root)`.
 */

async function git(args: string[], cwd?: string): Promise<void> {
  const { code } = await new Deno.Command('git', {
    args,
    cwd,
    stdout: 'inherit',
    stderr: 'inherit',
  }).output();
  if (code !== 0) throw new Error(`git ${args.join(' ')} failed (exit ${code})`);
}

async function gitCapture(args: string[]): Promise<string> {
  const { code, stdout } = await new Deno.Command('git', {
    args,
    stdout: 'piped',
    stderr: 'inherit',
  }).output();
  if (code !== 0) throw new Error(`git ${args.join(' ')} failed (exit ${code})`);
  return new TextDecoder().decode(stdout).trim();
}

async function denoTask(name: string): Promise<void> {
  const { code } = await new Deno.Command('deno', {
    args: ['task', name],
    stdout: 'inherit',
    stderr: 'inherit',
  }).output();
  if (code !== 0) throw new Error(`deno task ${name} failed (exit ${code})`);
}

async function rmrf(path: string): Promise<void> {
  try {
    await Deno.remove(path, { recursive: true });
  } catch (e) {
    if (!(e instanceof Deno.errors.NotFound)) throw e;
  }
}

// 1. Full production build → _site/ (wasm, css, alpine, rules, jekyll, sw, minify).
console.log('[deploy] building…');
await denoTask('build');

// 2. Serve _site as-is (no server-side Jekyll re-render).
await Deno.writeTextFile('_site/.nojekyll', '');

// 3. Ensure the custom-domain file is present (Jekyll copies root CNAME; be safe).
try {
  await Deno.stat('_site/CNAME');
} catch {
  await Deno.copyFile('CNAME', '_site/CNAME');
}

// 4. Publish _site/ as a single fresh commit, force-pushed to gh-pages.
const origin = await gitCapture(['remote', 'get-url', 'origin']);
await rmrf('_site/.git');
await git(['init', '-q', '-b', 'gh-pages'], '_site');
await git(['add', '-A'], '_site');
await git(
  ['-c', 'user.name=deploy', '-c', 'user.email=deploy@local', 'commit', '-q', '-m', 'Deploy site'],
  '_site',
);
await git(['push', '-q', '-f', origin, 'gh-pages:gh-pages'], '_site');
await rmrf('_site/.git');

console.log('[deploy] published _site/ → gh-pages');
