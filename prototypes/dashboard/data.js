window.CIPHERA=(()=>{
const days=[],long=[];const wd=['dim.','lun.','mar.','mer.','jeu.','ven.','sam.'];const mo=['janv.','févr.','mars','avr.','mai','juin'];
for(let i=0;i<30;i++){const d=new Date(2026,3,11+i);days.push(d.getDate()+' '+mo[d.getMonth()]);long.push(wd[d.getDay()]+' '+d.getDate()+' '+mo[d.getMonth()]);}
const deploys=[38,41,44,40,36,30,43,47,49,46,45,48,52,50,51,53,47,44,49,55,54,56,58,57,60,62,66,58,63,71];
const latency=[142,150,148,155,160,171,158,149,152,146,151,149,145,147,152,158,190,176,160,154,150,148,151,149,146,152,155,149,182,176];
const periods={
'7 derniers jours':{range:'9 mai - 15 mai 2026',kpi:[['412','6.1%','up'],['98.1%','0.4%','up'],['38 s','5.2%','up']],dep:'71 / jour',depDelta:'18.3%',cpu:.46,ram:'61 %',containers:'184',online:.88},
'30 derniers jours':{range:'16 avr. - 15 mai 2026',kpi:[['1 486','12.4%','up'],['97.3%','0.6%','up'],['42 s','3.1%','up']],dep:'71 / jour',depDelta:'27.4%',cpu:.52,ram:'64 %',containers:'184',online:.88},
'90 derniers jours':{range:'15 févr. - 15 mai 2026',kpi:[['3 902','31.0%','up'],['96.8%','0.9%','down'],['47 s','8.4%','up']],dep:'71 / jour',depDelta:'44.2%',cpu:.49,ram:'59 %',containers:'184',online:.88}};
const servers=[
{id:'srv-par-01',name:'srv-par-01',region:'Paris · eu-west-3',status:['ok','En ligne'],cpu:38,ram:'5,2 / 8 Go',containers:42,agent:'0.9.4',ip:'51.15.204.12',os:'Debian 12',cores:4,disk:'61 / 160 Go',cert:'expire dans 21 j',rtt:'4 ms'},
{id:'srv-par-02',name:'srv-par-02',region:'Paris · eu-west-3',status:['pending','Charge élevée'],cpu:87,ram:'7,1 / 8 Go',containers:57,agent:'0.9.4',ip:'51.15.204.13',os:'Ubuntu 24.04',cores:4,disk:'118 / 160 Go',cert:'expire dans 21 j',rtt:'5 ms'},
{id:'srv-fra-01',name:'srv-fra-01',region:'Francfort · eu-central-1',status:['ok','En ligne'],cpu:27,ram:'2,1 / 4 Go',containers:19,agent:'0.9.4',ip:'18.196.40.9',os:'Ubuntu 24.04',cores:2,disk:'22 / 80 Go',cert:'expire dans 26 j',rtt:'11 ms'},
{id:'srv-iad-01',name:'srv-iad-01',region:'Virginie · us-east-1',status:['error','Hors ligne'],cpu:0,ram:'— / 16 Go',containers:0,agent:'0.9.2',ip:'34.201.18.77',os:'Fedora 41',cores:8,disk:'204 / 320 Go',cert:'expire dans 3 j',rtt:'—'}];
const projects=[
{id:'web',name:'ciphera-web',env:'Production',status:['ok','En ligne'],services:3,server:'srv-par-01',build:'Railpack',branch:'main',updated:'il y a 4 min',repo:'github.com/ciphera/web'},
{id:'billing',name:'billing-api',env:'Production',status:['error','Échec du build'],services:2,server:'srv-par-02',build:'Dockerfile',branch:'main',updated:'il y a 1 h',repo:'github.com/ciphera/billing'},
{id:'dash',name:'dashboard-v2',env:'Preview',status:['pending','Build en cours'],services:1,server:'srv-fra-01',build:'Railpack',branch:'feat/charts',updated:'il y a 2 min',repo:'github.com/ciphera/dashboard'},
{id:'auth',name:'auth-service',env:'Production',status:['ok','En ligne'],services:2,server:'srv-par-01',build:'Dockerfile',branch:'main',updated:'il y a 2 j',repo:'github.com/ciphera/auth'},
{id:'mkt',name:'marketing-site',env:'Staging',status:['ok','En ligne'],services:1,server:'srv-fra-01',build:'Compose',branch:'develop',updated:'il y a 5 j',repo:'gitlab.com/ciphera/site'}];
const services={
web:[{n:'web',t:'Application',img:'Railpack · Node 22',s:['ok','En ligne'],rep:'2 / 2',port:'3000'},{n:'worker',t:'Worker',img:'Railpack · Node 22',s:['ok','En ligne'],rep:'1 / 1',port:'—'},{n:'postgres',t:'Base de données',img:'postgres:17',s:['ok','En ligne'],rep:'1 / 1',port:'5432'}],
billing:[{n:'api',t:'Application',img:'Dockerfile · Rust 1.83',s:['error','Échec du build'],rep:'0 / 2',port:'8080'},{n:'redis-free',t:'File de tâches',img:'PostgreSQL SKIP LOCKED',s:['ok','En ligne'],rep:'1 / 1',port:'—'}],
dash:[{n:'web',t:'Application',img:'Railpack · Vite',s:['pending','Build en cours'],rep:'0 / 1',port:'4173'}],
auth:[{n:'api',t:'Application',img:'Dockerfile · Rust 1.83',s:['ok','En ligne'],rep:'2 / 2',port:'8443'},{n:'postgres',t:'Base de données',img:'postgres:17',s:['ok','En ligne'],rep:'1 / 1',port:'5432'}],
mkt:[{n:'site',t:'Application',img:'Compose · nginx',s:['ok','En ligne'],rep:'1 / 1',port:'80'}]};
const deployments=[
{id:'#4821',commit:'a3f91c2',msg:'feat: graphiques de latence',s:['pending','En cours'],author:'Alex Martin',dur:'—',when:'il y a 2 min'},
{id:'#4818',commit:'7be0d14',msg:'fix: cache des en-têtes',s:['ok','Réussi'],author:'Lina Moreau',dur:'38 s',when:'il y a 4 h'},
{id:'#4812',commit:'c41a9e0',msg:'chore: mise à jour Node 22',s:['ok','Réussi'],author:'Yanis Benali',dur:'52 s',when:'hier'},
{id:'#4807',commit:'09d2bb7',msg:'feat: page tarifs',s:['error','Échec'],author:'Alex Martin',dur:'14 s',when:'il y a 2 j'}];
const buildLog=[
{time:'12:04:31',text:'$ ciphera deploy --project ciphera-web',tone:'cmd'},
{time:'12:04:31',text:'Agent srv-par-01 connecté (QUIC · mTLS)',tone:'muted'},
{time:'12:04:32',text:'Clonage github.com/ciphera/web@a3f91c2',tone:'info'},
{time:'12:04:34',text:'Railpack : Node 22 détecté, pnpm 9',tone:'info'},
{time:'12:04:35',text:'BuildKit : 14 / 18 couches en cache',tone:'muted'},
{time:'12:04:49',text:'Image poussée : registry.example.com/web:4821',tone:'info'},
{time:'12:04:51',text:'Healthcheck GET /health → 200',tone:'ok'},
{time:'12:04:52',text:'Trafic basculé via Caddy, ancienne version arrêtée',tone:'ok'}];
return {days,long,deploys,latency,periods,servers,projects,services,deployments,buildLog};
})();
