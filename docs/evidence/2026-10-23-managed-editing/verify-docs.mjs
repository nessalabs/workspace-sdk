import {fileURLToPath} from 'node:url';
import {writeFile} from 'node:fs/promises';
const renderer=process.env.NESSA_DOCS_ROOT || '/workspace/nessa-docs';
const project=fileURLToPath(new URL('../../../',import.meta.url));
const out=fileURLToPath(new URL('.',import.meta.url));
const {chromium}=await import(renderer+'/node_modules/playwright/index.mjs');
const {startVerificationServer}=await import(renderer+'/verification/server.mjs');
const {waitForDocument,waitForDiagrams}=await import(renderer+'/verification/lib.mjs');
const server=await startVerificationServer(project);
const checks=[];const errors=[];let browser;
function check(name,pass){checks.push({name,pass:!!pass});if(!pass)throw new Error(name);}
try {
 browser=await chromium.launch({executablePath:process.env.CHROMIUM_PATH || '/usr/bin/chromium',args:['--no-sandbox']});
 const page=await browser.newPage({viewport:{width:1440,height:1000}});page.on('pageerror',e=>errors.push(String(e)));
 await page.goto(server.url+'/docs/workspace-sdk');await waitForDocument(page,'workspace-sdk');await waitForDiagrams(page);
 const navigation=page.getByRole('link',{name:'Open Stage and publish managed local edits'});
 check('root map links to managed editing',await navigation.count()===1);await navigation.click();await waitForDocument(page,'managed-editing');await waitForDiagrams(page);
 check('both managed edit charts render',await page.locator('figure.diagram[data-state="rendered"]').count()===2);
 await page.getByTestId('source-revision').click();check('sourceRevision exact reviewed commit',(await page.getByTestId('source-revision-full').innerText()).includes('ec32735865d41e6b5d320ffaed6a9c8f91bedf46'));await page.keyboard.press('Escape');
 await page.getByRole('button',{name:'Details: Candidate',exact:true}).click();check('candidate explanation describes isolated staging',(await page.locator('dialog[open]').innerText()).includes('Every session gets its own revision namespace'));await page.keyboard.press('Escape');
 await page.getByRole('button',{name:'Details: Interrupted',exact:true}).click();check('interrupted explanation preserves recovery',(await page.locator('dialog[open]').innerText()).includes('Inspect the engine'));await page.keyboard.press('Escape');
 const conflict=page.locator('svg [role="button"]').filter({hasText:'Conflicted'});const labels=await page.locator('svg [role="button"]').evaluateAll(xs=>xs.map(x=>x.getAttribute('aria-label')));const conflictLabel=labels.find(x=>x?.startsWith('Details: Conflicted'));
 check('conflict node has existing owner drill-down',!!conflictLabel);await page.getByRole('button',{name:conflictLabel,exact:true}).click();const owner=page.locator('dialog[open] a[href^="/docs/file-sync"]');check('conflict detail links to existing checkpoint workflow',await owner.count()>0);await owner.first().click();await waitForDocument(page,'file-sync');await waitForDiagrams(page);check('existing checkpoint chart still renders',await page.locator('figure.diagram[data-state="rendered"]').count()>0);
 await page.goto(server.url+'/docs/managed-editing');await waitForDocument(page,'managed-editing');await waitForDiagrams(page);await page.screenshot({path:out+'/managed-edit-desktop.png',fullPage:true});
 await page.setViewportSize({width:390,height:844});await page.waitForTimeout(250);await page.evaluate(()=>document.querySelector('button[aria-label="Hide sidebar"]')?.click());for(const fit of await page.locator('[data-action="fit"]').all())await fit.click();await page.waitForTimeout(250);check('mobile no horizontal overflow',await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));await page.screenshot({path:out+'/managed-edit-mobile.png',fullPage:true});
 await page.goto(server.url+'/docs/workspace-files');await waitForDocument(page,'workspace-files');await waitForDiagrams(page);check('portable observation charts render',await page.locator('figure.diagram[data-state="rendered"]').count()===2);await page.goto(server.url+'/docs/workspace-files?view=details');await waitForDocument(page,'workspace-files');check('portable page names macOS',(await page.locator('body').innerText()).includes('macOS') || (await page.content()).includes('macOS'));
 check('no page errors',errors.length===0);
} catch(error){errors.push(String(error));process.exitCode=1;} finally{await browser?.close();server.stop();await writeFile(out+'/docs-browser.json',JSON.stringify({checks,errors,engine:'system chromium',sourceRevision:'ec32735865d41e6b5d320ffaed6a9c8f91bedf46'},null,2));}
