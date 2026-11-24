(()=>{
const {StatGroup,Stat,DataTable,StatusDot,AreaChart,KeyValueList,Button,Badge,Modal,TextField,RadioCard,Icon}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const mono=v=><span style={{fontFamily:'var(--font-mono)',fontSize:12}}>{v}</span>;
const dot=v=><StatusDot status={v[0]} label={v[1]}/>;
const Stats=({s})=><div style={{padding:'16px 32px 28px 46px',borderBottom:hair}}><StatGroup>{s.map((x,i)=><Stat key={i} label={x[0]} value={x[1]} delta={x[2]} direction={x[3]} deltaSuffix={x[2]?(x[4]||'vs 30 jours précédents'):undefined}/>)}</StatGroup></div>;
function ResourcePage({page}){
  const P=window.CIPHERA_PAGES[page];
  const cols=P.cols.map(([key,label,width,isStatus,align])=>({key,label,width:width||undefined,align:align||undefined,render:isStatus?dot:undefined}));
  const rows=P.rows.map(r=>Object.fromEntries(P.cols.map((c,i)=>[c[0],r[i]])));
  return <div><Stats s={P.stats}/><div style={{padding:'28px 32px 40px 34px'}}><DataTable columns={cols} rows={rows}/></div></div>;
}
function MonitoringPage(){
  const D=window.CIPHERA;
  return <div>
    <Stats s={[['Disponibilité','99.98%','0.02%','up'],['Latence p95','182 ms','4.3%','down'],['Erreurs 5xx / min','0.4','1.1%','up']]}/>
    <div style={{padding:'28px 32px 26px 46px',borderBottom:hair}}>
      <div style={{display:'flex',alignItems:'flex-end',justifyContent:'space-between',marginBottom:36}}><Stat size="lg" labelBelow label="Latence p95 · proxy Caddy" value="182 ms"/><span style={{font:'400 11px/1 var(--font-sans)',color:'var(--text-subtle)'}}>OpenTelemetry · export Prometheus</span></div>
      <AreaChart data={D.latency} yMax={200} yTicks={[200,150,100,50,0]} yFormat={v=>String(v)} series="p95" valueFormat={v=>Math.round(v)+' ms'} pointLabels={D.days} tooltipLabels={D.long} ticks={[0,6,11,17,23,29]} height={160}/>
    </div>
    <div style={{padding:'28px 32px 40px 34px'}}>
      <DataTable columns={[{key:'a',label:'Alerte',width:'1.6fr'},{key:'svc',label:'Cible'},{key:'s',label:'Statut',render:dot},{key:'w',label:'Déclenchée',align:'right'}]} rows={[{a:"Taux d'erreur > 2 % pendant 5 min",svc:'billing-api',s:['error','Active'],w:'il y a 1 h'},{a:'CPU > 85 %',svc:'srv-par-02',s:['pending','Surveillée'],w:'il y a 6 h'},{a:'Agent injoignable',svc:'srv-iad-01',s:['error','Active'],w:'il y a 3 h'},{a:'Certificat mTLS expire dans 3 j',svc:'srv-iad-01',s:['pending','Surveillée'],w:'il y a 1 j'},{a:'Healthcheck en échec',svc:'auth-service',s:['off','Résolue'],w:'il y a 2 j'}]}/>
    </div>
  </div>;
}
const CONN=[{n:'GitHub',i:'github',t:'Dépôts Git',d:'Déploiement à chaque push, previews par pull request',s:['ok','Connecté'],sync:'il y a 3 min'},{n:'Docker Hub',i:'container',t:'Registre',d:'Pull d\'images privées',s:['ok','Connecté'],sync:'il y a 1 h'},{n:'S3 · Scaleway',i:'hard-drive',t:'Sauvegardes',d:'Dumps PostgreSQL chiffrés chaque nuit',s:['ok','Connecté'],sync:'il y a 6 h'}];
function ConnectorsPage(){
  return <div>
    <Stats s={[['Connecteurs','3'],['Synchronisations (24 h)','1 204'],['Erreurs (24 h)','0']]}/>
    <div style={{padding:'28px 32px 40px 34px'}}><DataTable columns={[{key:'n',label:'Connecteur',render:(v,r)=><span style={{display:'flex',alignItems:'center',gap:10}}><Icon name={r.i} size={15} color="var(--text-muted)"/>{v}</span>},{key:'t',label:'Type'},{key:'d',label:'Usage',width:'1.8fr'},{key:'s',label:'Statut',render:dot},{key:'sync',label:'Dernière synchro',align:'right'}]} rows={CONN}/></div>
  </div>;
}
function AddConnectorModal({onClose}){
  const [c,setC]=React.useState('gitlab');
  const opts=[['gitlab','gitlab','GitLab','Dépôts Git auto-hébergés ou gitlab.com'],['bitbucket','git-branch','Bitbucket','Dépôts Git Atlassian'],['ghcr','package','GitHub Container Registry','Registre OCI privé'],['slack','message-square','Slack','Notifications de déploiement et alertes'],['prom','activity','Prometheus','Export des métriques OpenTelemetry']];
  return <Modal title="Ajouter un connecteur" description="Les identifiants sont chiffrés et ne sont jamais renvoyés par l'API." onClose={onClose} width={540} footer={<><Button onClick={onClose}>Annuler</Button><Button variant="inverse" onClick={onClose}>Continuer</Button></>}>
    <div style={{display:'flex',flexDirection:'column',gap:8}}>{opts.map(o=><RadioCard key={o[0]} icon={o[1]} title={o[2]} description={o[3]} selected={c===o[0]} onClick={()=>setC(o[0])}/>)}</div>
  </Modal>;
}
function SettingsPage({openSecret}){
  const sec={padding:'32px 32px 32px 46px',borderBottom:hair,display:'grid',gridTemplateColumns:'240px minmax(0,600px)',gap:40};
  const H=({t,d})=><div style={{display:'flex',flexDirection:'column',gap:8}}><span style={{font:'500 14px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>{t}</span><span style={{font:'400 12px/1.45 var(--font-sans)',color:'var(--text-subtle)'}}>{d}</span></div>;
  return <div>
    <div style={sec}><H t="Organisation" d="Informations générales de l'espace de travail."/><KeyValueList rows={[{label:'Nom :',value:'Ciphera'},{label:'Identifiant :',value:mono('ciphera')},{label:'Région par défaut :',value:'Paris (eu-west-3)'},{label:'Fuseau horaire :',value:'Europe/Paris'}]}/></div>
    <div style={sec}><H t="Passkeys" d="Connexion sans mot de passe via WebAuthn."/><div style={{display:'flex',flexDirection:'column',gap:16}}>
      <DataTable columns={[{key:'n',label:'Appareil',render:(v,r)=><span style={{display:'flex',alignItems:'center',gap:10}}><Icon name={r.i} size={15} color="var(--text-muted)"/>{v}</span>},{key:'a',label:'Ajoutée'},{key:'u',label:'Dernière utilisation',align:'right'}]} rows={[{n:'MacBook Pro · Touch ID',i:'laptop',a:'12 janv. 2026',u:"aujourd'hui"},{n:'iPhone 16 · Face ID',i:'smartphone',a:'3 févr. 2026',u:'hier'},{n:'YubiKey 5C',i:'key-round',a:'3 févr. 2026',u:'il y a 2 sem.'}]}/>
      <div><Button icon="plus">Ajouter une passkey</Button></div></div></div>
    <div style={sec}><H t="Double authentification" d="Code TOTP en complément ou en secours des passkeys."/><KeyValueList rows={[{label:'TOTP :',value:<Badge tone="positive">Activé</Badge>},{label:'Obligatoire pour les membres :',value:'Oui'},{label:'Codes de secours :',value:'8 restants'}]}/></div>
    <div style={sec}><H t="Secrets de l'organisation" d="Chiffrement enveloppe (age). Valeurs en écriture seule."/><div style={{display:'flex',flexDirection:'column',gap:16}}>
      <DataTable columns={[{key:'k',label:'Nom',render:mono},{key:'p',label:'Portée'},{key:'u',label:'Modifié',align:'right'}]} rows={[{k:'REGISTRY_TOKEN',p:'Tous les projets',u:'il y a 4 j'},{k:'S3_BACKUP_KEY',p:'Bases de données',u:'il y a 1 mois'},{k:'SENTRY_DSN',p:'ciphera-web, auth-service',u:'il y a 2 mois'}]}/>
      <div><Button icon="plus" onClick={openSecret}>Ajouter un secret</Button></div></div></div>
    <div style={{...sec,borderBottom:'none'}}><H t="Zone de danger" d="Actions irréversibles."/><div><Button>Supprimer l'organisation</Button></div></div>
  </div>;
}
function SecretModal({onClose}){
  return <Modal title="Ajouter un secret" description="La valeur est chiffrée immédiatement. Elle ne pourra plus être lue, seulement remplacée." onClose={onClose} footer={<><Button onClick={onClose}>Annuler</Button><Button variant="inverse" onClick={onClose}>Enregistrer</Button></>}>
    <TextField label="Nom" mono placeholder="API_KEY"/><TextField label="Valeur" mono type="password" placeholder="••••••••"/><TextField label="Portée" defaultValue="Tous les projets"/>
  </Modal>;
}
Object.assign(window,{ResourcePage,MonitoringPage,ConnectorsPage,AddConnectorModal,SettingsPage,SecretModal});
})();
