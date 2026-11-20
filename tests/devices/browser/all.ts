/**
 * Every browser check, one after another, against one site: the one named
 * on the command line (the live one, say), or else the built site, served
 * here.
 *
 *   deno task browser [site]          (BROWSER=firefox for Firefox)
 */
import { run as back } from './back.ts';
import { BROWSER, Checks, serveBuiltSite } from './site.ts';
import { run as switchCamera } from './switch.ts';
import { run as sync } from './sync.ts';
import { run as tabs } from './tabs.ts';

const given = Deno.args[0];
const served = given ? null : serveBuiltSite();
const site = given || served!.site;
const checks = new Checks();
const all = [['back', back], ['switch', switchCamera], ['tabs', tabs], ['sync', sync]] as const;
for (const [name, run] of all) {
  try {
    await run(site, checks);
  } catch (error) {
    checks.check(`${name}: the run finished`, false, String(error));
  }
}
if (served) await served.stop();
console.log(`${checks.passed}/${checks.results.length} checks passed (${BROWSER}, ${site})`);
Deno.exit(checks.allPassed ? 0 : 1);
