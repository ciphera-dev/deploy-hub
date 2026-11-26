(()=>{
const {StatGroup,Stat,DataTable,Tabs,Button,IconButton,Badge,Modal,TextField,Switch,Icon}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const mono=(v,c)=><span style={{fontFamily:'var(--font-mono)',fontSize:12,color:c}}>{v}</span>;
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const ENVS=['Production','Preview','Staging'];
const PROJ=['ciphera-web','billing-api','dashboard-v2','auth-service','marketing-site'];
const INIT=[
  {k:'SENTRY_DSN',v:'https://8f2c…@o4501.ingest.sentry.io/77',secret:false,envs:['Production','Preview','Staging'],proj:'all',upd:'il y a 3 j',by:'Alex Martin'},
  {k:'RESEND_API_KEY',v:'',secret:true,envs:['Production'],proj:'all',upd:'il y a 12 j',by:'Lina Moreau'},
  {k:'S3_BUCKET',v:'ciphera-assets-eu',secret:false,envs:['Production','Staging'],proj:['ciphera-web','marketing-site'],upd:'il y a 1 mois',by:'Alex Martin'},
  {k:'S3_SECRET_ACCESS_KEY',v:'',secret:true,envs:['Production','Staging'],proj:['ciphera-web','marketing-site'],upd:'il y a 1 mois',by:'Alex Martin'},
  {k:'LOG_LEVEL',v:'info',secret:false,envs:['Production'],proj:'all',upd:'il y a 2 mois',by:'Yanis Benali'},
  {k:'LOG_LEVEL',v:'debug',secret:false,envs:['Preview','Staging'],proj:'all',upd:'il y a 2 mois',by:'Yanis Benali'},
  {k:'STRIPE_WEBHOOK_SECRET',v:'',secret:true,envs:['Production'],proj:['billing-api'],upd:'il y a 6 j',by:'Yanis Benali'},
  {k:'OTEL_EXPORTER_OTLP_ENDPOINT',v:'http://otel.internal:4317',secret:false,envs:['Production','Preview','Staging'],proj:'all',upd:'il y a 3 sem.',by:'Alex Martin'},
];
let uid=0;const withId=r=>({...r,id:++uid});

function VarModal({initial,onClose,onSave}){
  const [k,setK]=React.useState(initial?.k||'');const [v,setV]=React.useState(initial&&!initial.secret?initial.v:'');
  const [secret,setSecret]=React.useState(initial?.secret??false);
  const [envs,setEnvs]=React.useState(initial?.envs||[...ENVS]);
  const [all,setAll]=React.useState(!initial||initial.proj==='all');
  const [proj,setProj]=React.useState(Array.isArray(initial?.proj)?initial.proj:[]);
  const tog=(arr,set,x)=>set(arr.includes(x)?arr.filter(y=>y!==x):[...arr,x]);
  const chip=(on,txt,onClick)=><button key={txt} onClick={onClick} style={{display:'flex',alignItems:'center',gap:6,height:28,padding:'0 10px',borderRadius:99,cursor:'pointer',font:'500 12px/1 var(--font-sans)',border:'1px solid '+(on?'var(--gray-500)':'var(--border-strong)'),background:on?'var(--surface-selected)':'transparent',color:on?'var(--text-strong)':'var(--text-muted)'}}>{on&&<Icon name="check" size={11}/>}{txt}</button>;
  const ok=k&&(v||(initial&&initial.secret))&&envs.length&&(all||proj.length);
  return <Modal title={initial?'Modifier la variable':'Nouvelle variable globale'} description="Injectée au prochain déploiement des projets concernés. Une variable de projet du même nom a priorité." onClose={onClose} width={560}
    footer={<><Button onClick={onClose}>Annuler</Button><Button variant="inverse" disabled={!ok} onClick={()=>onSave({k:k.toUpperCase().replace(/[^A-Z0-9_]/g,'_'),v:secret?'':v,secret,envs,proj:all?'all':proj,upd:"à l'instant",by:'Alex Martin'})}>Enregistrer</Button></>}>
    <div style={{display:'flex',flexDirection:'column',gap:18}}>
      <div style={{display:'flex',gap:12}}><TextField label="Clé" mono placeholder="API_URL" value={k} onChange={setK} style={{flex:1}}/><TextField label="Valeur" mono type={secret?'password':'text'} placeholder={initial?.secret?'Laisser vide pour conserver':'valeur'} value={v} onChange={setV} style={{flex:1.4}}/></div>
      <Switch checked={secret} onChange={setSecret} label="Secret" description="Chiffré (age) et en écriture seule : la valeur ne sera plus jamais affichée."/>
      <div style={{display:'flex',flexDirection:'column',gap:8}}><span style={lbl}>Environnements</span><div style={{display:'flex',gap:6}}>{ENVS.map(e=>chip(envs.includes(e),e,()=>tog(envs,setEnvs,e)))}</div></div>
      <div style={{display:'flex',flexDirection:'column',gap:8}}><span style={lbl}>Projets</span>
        <div style={{display:'flex',gap:6,flexWrap:'wrap'}}>{chip(all,'Tous les projets',()=>setAll(true))}{chip(!all,'Sélection',()=>setAll(false))}</div>
        {!all&&<div style={{display:'flex',gap:6,flexWrap:'wrap',paddingTop:4}}>{PROJ.map(p=>chip(proj.includes(p),p,()=>tog(proj,setProj,p)))}</div>}
      </div>
    </div>
  </Modal>;
}

function ImportModal({onClose,onImport}){
  const [txt,setTxt]=React.useState('# Collez le contenu d\'un fichier .env\nAPI_URL=https://api.example.com\nFEATURE_FLAGS=charts,logs\n');
  const [secret,setSecret]=React.useState(false);
  const rows=txt.split('\n').map(l=>l.trim()).filter(l=>l&&!l.startsWith('#')&&l.includes('=')).map(l=>{const i=l.indexOf('=');return [l.slice(0,i).trim(),l.slice(i+1).trim().replace(/^["']|["']$/g,'')]});
  return <Modal title="Importer un fichier .env" description={rows.length+' variable(s) détectée(s), appliquées à tous les projets et environnements.'} onClose={onClose} width={560}
    footer={<><Button onClick={onClose}>Annuler</Button><Button variant="inverse" disabled={!rows.length} onClick={()=>onImport(rows.map(([k,v])=>({k,v:secret?'':v,secret,envs:[...ENVS],proj:'all',upd:"à l'instant",by:'Alex Martin'})))}>Importer {rows.length}</Button></>}>
    <div style={{display:'flex',flexDirection:'column',gap:16}}>
      <textarea value={txt} onChange={e=>setTxt(e.target.value)} spellCheck="false" style={{height:180,resize:'vertical',padding:12,borderRadius:'var(--radius-lg)',border:'1px solid var(--border-strong)',background:'var(--gray-0)',color:'var(--text-body)',font:'400 12px/1.6 var(--font-mono)',outline:'none'}}></textarea>
      <Switch checked={secret} onChange={setSecret} label="Importer comme secrets" description="Toutes les valeurs seront chiffrées et masquées."/>
    </div>
  </Modal>;
}

function GRow({r,i,cols,EC,rev,toggle,edit,copy,del,used}){
  const [h,setH]=React.useState(false);
  return <div onMouseEnter={()=>setH(true)} onMouseLeave={()=>setH(false)} style={{display:'grid',gridTemplateColumns:cols,gap:16,alignItems:'center',minHeight:54,padding:'0 12px 0 18px',borderTop:i?'1px solid rgba(255,255,255,.06)':'none',background:h?'rgba(255,255,255,.02)':'transparent'}}>
    <span style={{display:'flex',alignItems:'center',gap:10,minWidth:0}}><span style={{width:26,height:26,borderRadius:7,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)',border:'1px solid var(--border-subtle)'}}><Icon name={r.secret?'lock':'braces'} size={12} color="var(--text-body)"/></span><span style={{font:'500 13px/1 var(--font-mono)',color:'var(--text-strong)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{r.k}</span></span>
    {r.secret?<span style={{display:'flex',alignItems:'center',gap:6,font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'}}><Icon name="eye-off" size={12}/>Écriture seule</span>:<span style={{display:'flex',alignItems:'center',gap:8,minWidth:0}}><span style={{flex:1,minWidth:0,height:28,display:'flex',alignItems:'center',padding:'0 10px',borderRadius:8,background:'rgba(255,255,255,.035)',font:'400 12px/1 var(--font-mono)',color:'var(--text-body)',overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{rev?r.v:'••••••••••••'}</span><button onClick={toggle} style={{background:'none',border:0,padding:0,cursor:'pointer',color:'var(--text-subtle)',display:'flex'}}><Icon name={rev?'eye-off':'eye'} size={14}/></button></span>}
    <span style={{display:'inline-flex',width:'fit-content',padding:2,gap:2,borderRadius:8,background:'var(--surface-raised)',border:'1px solid var(--border-subtle)'}}>{['Production','Preview','Staging'].map(e=>{const on=r.envs.includes(e);return <span key={e} title={e} style={{height:20,display:'flex',alignItems:'center',padding:'0 7px',borderRadius:6,background:on?'var(--surface-selected)':'transparent',font:'500 11px/1 var(--font-sans)',color:on?'var(--text-strong)':'var(--text-faint)'}}>{e==='Production'?'Prod':e}</span>})}</span>
    <span style={{display:'flex',alignItems:'center',gap:6,font:'400 12px/1 var(--font-sans)',color:'var(--text-body)'}}><Icon name="layers" size={12} color="var(--text-subtle)"/>{r.proj==='all'?'Tous':used}</span>
    <span style={{display:'flex',flexDirection:'column',gap:5}}><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-body)'}}>{r.upd}</span><span style={{...lbl,fontSize:11}}>{r.by.split(' ')[0]}</span></span>
    <div style={{display:'flex',gap:2,justifyContent:'flex-end',opacity:h?1:.5,transition:'opacity .15s'}}><IconButton size={28} variant="ghost" icon="pencil" label="Modifier" onClick={edit}/>{!r.secret&&<IconButton size={28} variant="ghost" icon="copy" label="Copier" onClick={copy}/>}<IconButton size={28} variant="ghost" icon="trash-2" label="Supprimer" onClick={del}/></div>
  </div>;
}
function GlobalVariablesPage({addOpen,closeAdd}){
  const [rows,setRows]=React.useState(()=>INIT.map(withId));
  const [env,setEnv]=React.useState('Tous');
  const [q,setQ]=React.useState('');
  const [reveal,setReveal]=React.useState({});
  const [modal,setModal]=React.useState(null);
  const [toast,setToast]=React.useState('');
  React.useEffect(()=>{if(addOpen)setModal({type:'edit'})},[addOpen]);
  const close=()=>{setModal(null);closeAdd&&closeAdd()};
  const flash=m=>{setToast(m);setTimeout(()=>setToast(''),1800)};
  const list=rows.filter(r=>(env==='Tous'||r.envs.includes(env))&&r.k.toLowerCase().includes(q.toLowerCase()));
  const save=data=>{setRows(rs=>modal.row?rs.map(r=>r.id===modal.row.id?{...r,...data,v:data.secret&&!data.v?r.v:data.v}:r):[withId(data),...rs]);flash(modal.row?'Variable mise à jour':'Variable ajoutée');close()};
  const exportEnv=()=>{const txt=list.map(r=>`${r.k}=${r.secret?'':r.v}${r.secret?'  # secret (valeur non exportée)':''}`).join('\n')+'\n';const a=document.createElement('a');a.href=URL.createObjectURL(new Blob([txt],{type:'text/plain'}));a.download='ciphera-globales'+(env==='Tous'?'':'-'+env.toLowerCase())+'.env';document.body.appendChild(a);a.click();a.remove();flash(list.length+' variables exportées')};
  const used=r=>r.proj==='all'?PROJ.length:r.proj.length;
  const box={borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',background:'var(--surface-app)'};const line='1px solid rgba(255,255,255,.06)';
  const EC={Production:'134,239,172',Preview:'167,139,250',Staging:'251,191,36'};
  const wb=(ic,t,f,solid)=><button onClick={f} style={{display:'flex',alignItems:'center',gap:8,height:38,padding:'0 15px',borderRadius:10,border:solid?0:'1px solid var(--border-strong)',cursor:'pointer',background:solid?'#8b7cf6':'transparent',color:solid?'#fff':'var(--text-strong)',font:'600 13px/1 var(--font-sans)'}}><Icon name={ic} size={14} color={solid?'#fff':'var(--text-strong)'}/>{t}</button>;
  const COLS='minmax(0,1.4fr) minmax(0,1.5fr) minmax(0,1.3fr) 110px 110px 96px';
  return <div style={{padding:'20px 32px 40px 34px',display:'flex',flexDirection:'column',gap:16}}>
    <div style={{borderRadius:20,overflow:'hidden',border:'1px solid var(--border-default)',background:'var(--surface-app)'}}>
      <div style={{position:'relative',display:'flex',alignItems:'center',gap:18,padding:'20px 24px',background:'transparent',overflow:'hidden'}}>
        <span style={{position:'relative',width:50,height:50,borderRadius:14,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'linear-gradient(145deg,#4a4a52,#26262c)',boxShadow:'inset 0 1px 0 rgba(255,255,255,.18)'}}><Icon name="braces" size={22} color="#f2f2f2"/></span>
        <span style={{position:'relative',display:'flex',flexDirection:'column',gap:8,flex:1,minWidth:0}}><span style={{font:'700 18px/1.1 var(--font-sans)',color:'var(--text-strong)'}}>Variables globales</span><span style={{font:'400 13px/1.4 var(--font-sans)',color:'var(--text-muted)'}}>Partagées par tous vos projets · chiffrées au repos · injectées au prochain déploiement</span></span>
        <span style={{position:'relative',display:'flex',gap:8}}>{wb('upload','Importer .env',()=>setModal({type:'import'}))}{wb('download','Exporter',exportEnv)}{wb('plus','Nouvelle variable',()=>setModal({type:'edit'}),true)}</span>
      </div>
      <div style={{display:'grid',gridTemplateColumns:'repeat(4,minmax(0,1fr))'}}>
        {[['Variables',rows.length,'braces'],['Secrets',rows.filter(r=>r.secret).length,'lock'],['Projets concernés',PROJ.length+' / '+PROJ.length,'layers'],['Dernière modification',rows.map(r=>r.upd)[0]||'—','clock']].map(([k,v,ic],i)=><div key={k} style={{display:'flex',flexDirection:'column',gap:9,padding:'16px 20px',borderLeft:i?line:'none'}}><span style={{display:'flex',alignItems:'center',gap:6,...lbl,whiteSpace:'nowrap'}}><Icon name={ic} size={12}/>{k}</span><span style={{font:'600 20px/1 var(--font-sans)',color:'var(--text-strong)',whiteSpace:'nowrap'}}>{v}</span></div>)}
      </div>
    </div>
    <div style={{display:'flex',alignItems:'center',gap:12,padding:'12px 16px',borderRadius:12,background:'var(--surface-raised)'}}>
      <Icon name="git-merge" size={15} color="var(--text-muted)"/>
      <span style={{font:'400 12px/1.5 var(--font-sans)',color:'var(--text-muted)'}}>Priorité au déploiement :</span>
      {['Variable du projet','Globale de l\'environnement','Globale tous environnements'].map((t,i)=><React.Fragment key={t}>{i>0&&<Icon name="chevron-right" size={12} color="var(--text-faint)"/>}<span style={{height:24,display:'flex',alignItems:'center',padding:'0 9px',borderRadius:99,background:'rgba(255,255,255,.05)',border:'1px solid rgba(255,255,255,.08)',font:'500 11px/1 var(--font-sans)',color:'var(--text-strong)'}}>{i+1}. {t}</span></React.Fragment>)}
    </div>
    <div style={{display:'flex',alignItems:'center',gap:22,borderBottom:'1px solid rgba(255,255,255,.08)'}}>
      {['Tous',...ENVS].map(e=>{const on=env===e;const n=e==='Tous'?rows.length:rows.filter(r=>r.envs.includes(e)).length;return <button key={e} onClick={()=>setEnv(e)} style={{display:'flex',alignItems:'center',gap:7,height:38,padding:0,border:0,borderBottom:'2px solid '+(on?'#ededed':'transparent'),marginBottom:-1,background:'transparent',cursor:'pointer',font:'500 13px/1 var(--font-sans)',color:on?'var(--text-strong)':'var(--text-muted)'}}>{e}<span style={{fontWeight:400,color:'var(--text-subtle)'}}>{n}</span></button>})}
      <span style={{flex:1}}></span>
      <div style={{display:'flex',alignItems:'center',gap:8,height:34,width:260,padding:'0 12px',marginBottom:4,borderRadius:10,border:'1px solid rgba(255,255,255,.1)'}}><Icon name="search" size={14} color="var(--text-subtle)"/><input value={q} onChange={e=>setQ(e.target.value)} placeholder="Rechercher une clé…" style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 13px/1 var(--font-mono)'}}/></div>
    </div>
    <div style={{...box,overflow:'hidden'}}>
      <div style={{display:'grid',gridTemplateColumns:COLS,gap:16,padding:'12px 18px',borderBottom:line,font:'500 12px/1 var(--font-sans)',color:'var(--text-strong)'}}><span>Clé</span><span>Valeur</span><span>Environnements</span><span>Projets</span><span>Modifiée</span><span></span></div>
      {list.map((r,i)=><GRow key={r.id} r={r} i={i} cols={COLS} EC={EC} rev={!!reveal[r.id]} toggle={()=>setReveal(x=>({...x,[r.id]:!x[r.id]}))} edit={()=>setModal({type:'edit',row:r})} copy={()=>{navigator.clipboard&&navigator.clipboard.writeText(r.k+'='+r.v);flash('Copié')}} del={()=>{setRows(rs=>rs.filter(x=>x.id!==r.id));flash(r.k+' supprimée')}} used={used(r)}/>)}
      {!list.length&&<div style={{display:'flex',flexDirection:'column',alignItems:'center',gap:8,padding:'40px 0'}}><Icon name="braces" size={18} color="var(--text-subtle)"/><span style={lbl}>Aucune variable.</span></div>}
    </div>
    {modal?.type==='edit'&&<VarModal initial={modal.row} onClose={close} onSave={save}/>}
    {modal?.type==='import'&&<ImportModal onClose={close} onImport={rs=>{setRows(x=>[...rs.map(withId),...x]);flash(rs.length+' variables importées');close()}}/>}
    {toast&&<div style={{position:'fixed',bottom:24,right:24,padding:'10px 14px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',zIndex:50}}>{toast}</div>}
  </div>;
}
window.GlobalVariablesPage=GlobalVariablesPage;window.CIPHERA_GLOBALS=INIT;
})();
