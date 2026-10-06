// Synthetic HTTP fixtures for the built pages. Postgres tests cover the actual import and permissions.
const assert = require('node:assert/strict'), fs = require('node:fs'), path = require('node:path'), http = require('node:http');
const {spawn} = require('node:child_process'), {createRequire} = require('node:module');
const webRequire = createRequire(path.resolve('apps/web/package.json'));
const {chromium} = require(process.argv[2] || 'playwright');
let settings = {title:'A new adventure',category_id:'coding',revision:0}, failed=false, saved, decision;
let invited;
const person=username=>({username,display_name:username,linked:true,deleted:false,avatar:null});
const squad={id:'fixture',mode:'SEPARATE',ended:false,host:true,joined:true,invited:false,members:[{...person('Wren'),host:true}],pending:[{username:'Oak',expires_at:'2026-12-01T00:00:00Z'}]};
const items=[{id:'coding',name:'Coding'},{id:'wikidata-q100',name:'Synthetic Frontier IV'}];
const genres=[{id:'fps_battle_royale',name:'FPS & battle royale'},{id:'mmos_rpgs',name:'MMOs & RPGs'},{id:'education_coding',name:'Education & coding'}];
let pending=[{id:'Q102',name:'Synthetic Hybrid',genres:['shooter game','role-playing video game'],description:'A game with two genres requiring review.'}];
const server=http.createServer(async(req,res)=>{
 const u=new URL(req.url,'http://localhost'), p=u.pathname;
 let text='';for await(const chunk of req)text+=chunk;const body=text?JSON.parse(text):{};
 let data;
 if(p==='/api/auth/me')data={username:'Wren',faction:'aetheron',email_verified:true,mfa_enabled:true,has_password:true,reauthenticated:true};
 else if(p==='/api/me/alerts')data={notifications:0};
 else if(p==='/api/me/following'||p==='/api/admin/appeals'||p==='/api/admin/streams')data={items:[]};
 else if(p==='/api/categories'){
  if(failed){res.writeHead(503,{'content-type':'application/json'});return res.end(JSON.stringify({error:'Catalog temporarily unavailable.'}));}
  const q=u.searchParams.get('q')?.toLowerCase()||'';
  if(q==='old'){await new Promise(r=>setTimeout(r,700));res.writeHead(200,{'content-type':'application/json'});return res.end(JSON.stringify({categories:[{id:'old',name:'Stale game'}]}));}
  data={categories:items.filter(i=>i.id===u.searchParams.get('include')||i.name.toLowerCase().includes(q)||q==='sf4'&&i.id==='wikidata-q100')};
 } else if(p==='/api/search'){
  const q=u.searchParams.get('q');
  if(q==='old'){await new Promise(r=>setTimeout(r,700));data={channels:[person('StaleStreamer')]};}
  else data={channels:q==='va'?[person('Wren'),person('Oak'),{...person('Vale'),display_name:'The Cartographer'},person('Vapor')]:[]};
 } else if(p==='/api/squads/fixture')data=squad;
 else if(p==='/api/squads/fixture/invites'){invited=body;data={invited:true};}
 else if(p.endsWith('/chat/moderation'))data={role:'owner',restrictions:[]};
 else if(p.endsWith('/chat'))data={messages:[],pinned:null,emotes:[]};
 else if(p.endsWith('/live'))data={live:false};
 else if(p==='/api/me/stream'){
  if(req.method==='PATCH'){saved=body;settings={...body,revision:settings.revision+1};data={revision:settings.revision,category_id:settings.category_id};}
  else data={configured:true,eligible:true,settings,disconnect_pending:false,credential:null,broadcast:null};
 } else if(p==='/api/admin/categories')data={items:[{...items[0],genre:'education_coding',active:true,channels:1}]};
 else if(p==='/api/admin/genres')data={items:genres};
 else if(p==='/api/admin/game-catalog')data={items:pending,status:{last_success_at:'2026-10-06T16:00:00Z',last_complete_at:null,failures:1,pending:pending.length}};
 else if(p==='/api/admin/game-catalog/Q102'){decision=body;pending=[];data={saved:true};}
 else {res.writeHead(404,{'content-type':'application/json'});return res.end('{}');}
 res.writeHead(200,{'content-type':'application/json','cache-control':'no-store'});res.end(JSON.stringify(data));
});
(async()=>{let next,browser;try{
 fs.mkdirSync('tmp/catalog-screenshots',{recursive:true});
 await new Promise(resolve=>server.listen(8080,'127.0.0.1',resolve));
 next=spawn(process.execPath,[webRequire.resolve('next/dist/bin/next'),'start','--hostname','127.0.0.1','--port','13004'],{cwd:path.resolve('apps/web'),env:{...process.env,API_INTERNAL_ORIGIN:'http://127.0.0.1:8080'},stdio:'ignore',windowsHide:true});
 for(let i=0;i<80;i++){try{if((await fetch('http://127.0.0.1:13004/login')).ok)break;}catch{}await new Promise(r=>setTimeout(r,250));}
 browser=await chromium.launch({headless:true,channel:'msedge'});
 for(const width of [1440,390]){
  settings={title:'A new adventure',category_id:'coding',revision:0};failed=false;pending=[{id:'Q102',name:'Synthetic Hybrid',genres:['shooter game','role-playing video game'],description:'A game with two genres requiring review.'}];
  saved=undefined;invited=undefined;
  const context=await browser.newContext({viewport:{width,height:1000},hasTouch:width===390}),page=await context.newPage(),errors=[];
  page.on('pageerror',e=>errors.push(e.message));
  await context.addCookies([{name:'sver_dev',value:'synthetic-catalog',url:'http://127.0.0.1:13004'}]);
  await page.goto('http://127.0.0.1:13004/studio/stream');
  const games=page.getByRole('combobox',{name:'Find a game or category',exact:true});
  const staleGame=page.waitForResponse(r=>r.url().includes('q=old'));
  const oldGameRequest=page.waitForRequest(r=>r.url().includes('q=old'));
  await games.fill('old');await oldGameRequest;
  await games.fill('sf4');
  await page.getByRole('listbox').getByRole('option',{name:'Synthetic Frontier IV',exact:true}).waitFor();
  await staleGame;await page.waitForTimeout(60);
  assert.equal(await page.getByRole('option',{name:'Stale game',exact:true}).count(),0);
  await page.evaluate(()=>window.scrollTo(0,0));
  await page.screenshot({path:`tmp/catalog-screenshots/autocomplete-stream-${width}.png`,fullPage:true});
  if(width===390)await page.getByRole('listbox').getByRole('option',{name:'Synthetic Frontier IV',exact:true}).tap();
  else{await games.press('ArrowDown');assert(await games.getAttribute('aria-activedescendant'));await games.press('Enter');}
  assert.equal(saved,undefined,'Selecting a suggestion must not submit the form');
  assert.equal(await page.getByRole('combobox',{name:/^Category/}).inputValue(),'wikidata-q100');
  await page.getByRole('textbox',{name:/^Title/}).fill('Unsaved title');
  await page.getByRole('button',{name:'Save details',exact:true}).click();
  await page.getByText('Stream details saved.',{exact:true}).waitFor();
  assert.equal(saved.category_id,'wikidata-q100');assert.equal(saved.title,'Unsaved title');
  failed=true;await page.getByLabel('Find a game or category').fill('absent');
  await page.getByRole('button',{name:'Retry catalog',exact:true}).waitFor();
  assert.equal(await page.getByRole('combobox',{name:/^Category/}).inputValue(),'wikidata-q100');
  failed=false;await page.getByRole('button',{name:'Retry catalog',exact:true}).click();
  await page.getByRole('button',{name:'Retry catalog',exact:true}).waitFor({state:'detached'});
  await page.getByLabel('Find a game or category').fill('sf4');
  await page.evaluate(()=>window.scrollTo(0,0));
  await page.screenshot({path:`tmp/catalog-screenshots/studio-${width}.png`,fullPage:true});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));
  await page.goto('http://127.0.0.1:13004/admin/categories');
  await page.getByText('1 games need classification.').waitFor();
  await page.screenshot({path:`tmp/catalog-screenshots/admin-${width}.png`,fullPage:true});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));
  await page.getByRole('combobox',{name:/^Genre for Synthetic Hybrid/}).selectOption('mmos_rpgs');
  await page.getByLabel('Reason for Synthetic Hybrid',{exact:true}).fill('Published primary genre reviewed.');
  await page.getByRole('button',{name:'Add game',exact:true}).click();
  await page.getByText('0 games need classification.').waitFor();assert.equal(decision.genre,'mmos_rpgs');
  await page.goto('http://127.0.0.1:13004/squads/fixture');
  const names=page.getByRole('combobox',{name:'Live streamer username',exact:true});
  const staleName=page.waitForResponse(r=>r.url().includes('/api/search?q=old'));
  const oldNameRequest=page.waitForRequest(r=>r.url().includes('/api/search?q=old'));
  await names.fill('old');await oldNameRequest;await names.fill('@va');
  const match=page.getByRole('option',{name:'The Cartographer · @Vale',exact:true});await match.waitFor();
  await staleName;await page.waitForTimeout(60);
  assert.equal(await page.getByRole('option',{name:/StaleStreamer|@Wren|@Oak/}).count(),0);
  await names.press('Escape');assert.equal(await names.getAttribute('aria-expanded'),'false');
  await names.press('ArrowDown');await match.waitFor();
  await page.evaluate(()=>window.scrollTo(0,0));
  await page.screenshot({path:`tmp/catalog-screenshots/autocomplete-squad-${width}.png`,fullPage:true});
  if(width===390)await match.tap();else await names.press('Enter');
  assert.equal(await names.inputValue(),'Vale');assert.equal(invited,undefined,'Choosing a username must not send an invitation');
  await page.getByRole('button',{name:'Send invitation',exact:true}).click();
  await page.getByText('Saved.',{exact:true}).waitFor();assert.equal(invited.username,'Vale');
  await names.fill('zz');await page.getByText('No other matching channels.',{exact:true}).waitFor();
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));
  assert.deepEqual(errors,[]);await context.close();console.log(`${width}px: game/username suggestions, keyboard/touch selection, stale responses, save/invite, recovery, no overflow/errors.`);
 }
}finally{if(browser)await browser.close();if(next)next.kill();server.closeAllConnections();await new Promise(r=>server.close(r));}})().catch(e=>{console.error(e);process.exitCode=1;});
