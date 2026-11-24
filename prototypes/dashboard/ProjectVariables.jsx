(()=>{
const {Button,IconButton,Badge,Icon,Switch,Tabs,Modal}=window.CipheraDesignSystem;
const hair='1px solid var(--border-subtle)';
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const ENVS=['Production','Preview','Staging'];
let uid=0;const V=o=>({id:++uid,envs:[...ENVS],by:'Alex Martin',upd:'il y a 2 mois',...o});
const INIT=()=>[
  V({k:'DATABASE_URL',v:'postgres://web:••••@prod-main:5432/web',secret:true,envs:['Production'],upd:'il y a 3 j'}),
  V({k:'DATABASE_URL',v:'postgres://web:••••@staging:5432/web',secret:true,envs:['Preview','Staging'],upd:'il y a 3 j'}),
  V({k:'STRIPE_SECRET_KEY',v:'sk_live_••••',secret:true,envs:['Production'],upd:'il y a 12 j',by:'Yanis Benali'}),
  V({k:'NODE_ENV',v:'production'}),
  V({k:'PORT',v:'3000'}),
  V({k:'NEXT_PUBLIC_API_URL',v:'https://app.ciphera.dev/api',envs:['Production'],upd:'il y a 5 j',by:'Lina Moreau'}),
  V({k:'LOG_LEVEL',v:'warn',envs:['Production'],upd:'hier'}),
];
const field={height:32,padding:'0 10px',borderRadius:'var(--radius-md)',border:'1px solid var(--border-strong)',background:'var(--gray-0)',color:'var(--text-strong)',font:'400 12px/1 var(--font-mono)',outline:'none',minWidth:0};
const cell={minWidth:0,overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'};
const COLS='minmax(0,1.2fr) minmax(0,1.6fr) minmax(0,1.1fr) 120px 92px';

function EnvChips({value,onChange,small}){
  return <div style={{display:'flex',gap:4,flexWrap:'wrap'}}>{ENVS.map(e=>{const on=value.includes(e);return <button key={e} onClick={onChange?()=>onChange(on?value.filter(x=>x!==e):[...value,e]):undefined} style={{height:small?20:24,padding:'0 7px',borderRadius:6,border:'1px solid '+(on?'var(--border-strong)':'transparent'),background:on?'var(--surface-selected)':'transparent',color:on?'var(--text-strong)':'var(--text-faint)',font:'500 11px/1 var(--font-sans)',cursor:onChange?'pointer':'default',textDecoration:!on&&!onChange?'line-through':'none'}}>{e.slice(0,4)}{e.length>4?'.':''}</button>})}</div>;
}

function VarRow({r,editing,onEdit,onSave,onCancel,onDelete,reveal,setReveal,flash}){
  const [h,setH]=React.useState(false);
  const [k,setK]=React.useState(r.k),[v,setV]=React.useState(r.secret?'':r.v),[secret,setSecret]=React.useState(!!r.secret),[envs,setEnvs]=React.useState(r.envs);
  React.useEffect(()=>{if(editing){setK(r.k);setV(r.secret?'':r.v);setSecret(!!r.secret);setEnvs(r.envs)}},[editing]);
  if(editing)return <div style={{display:'grid',gridTemplateColumns:COLS,gap:12,alignItems:'center',padding:'10px 12px',borderBottom:hair,background:'var(--surface-raised)'}}>
    <input autoFocus value={k} onChange={e=>setK(e.target.value.toUpperCase().replace(/[^A-Z0-9_]/g,'_'))} placeholder="CLÉ" style={field}/>
    <div style={{display:'flex',gap:6,alignItems:'center'}}><input value={v} onChange={e=>setV(e.target.value)} type={secret?'password':'text'} placeholder={r.secret&&r.k?'Laisser vide pour conserver':'valeur'} style={{...field,flex:1}}/>
      <button onClick={()=>setSecret(s=>!s)} title="Secret" style={{width:32,height:32,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',borderRadius:'var(--radius-md)',border:'1px solid '+(secret?'var(--gray-500)':'var(--border-strong)'),background:secret?'var(--surface-selected)':'transparent',color:secret?'var(--text-strong)':'var(--text-subtle)',cursor:'pointer'}}><Icon name={secret?'lock':'lock-open'} size={13}/></button></div>
    <EnvChips value={envs} onChange={setEnvs}/>
    <span></span>
    <div style={{display:'flex',gap:4,justifyContent:'flex-end'}}><IconButton size={28} variant="ghost" icon="x" label="Annuler" onClick={onCancel}/><IconButton size={28} icon="check" label="Valider" onClick={()=>k&&envs.length&&onSave({k,v:secret&&!v?r.v:v,secret,envs})}/></div>
  </div>;
  const shown=r.secret?'••••••••••••':reveal?r.v:'•'.repeat(Math.min(12,Math.max(6,r.v.length)));
  return <div onMouseEnter={()=>setH(true)} onMouseLeave={()=>setH(false)} style={{display:'grid',gridTemplateColumns:COLS,gap:12,alignItems:'center',minHeight:52,padding:'0 12px',borderBottom:hair,background:h?'var(--surface-raised)':'transparent'}}>
    <div style={{...cell,display:'flex',alignItems:'center',gap:8}}>{r.secret?<Icon name="lock" size={12} color="var(--text-subtle)"/>:<span style={{width:12}}></span>}<span style={{...cell,font:'500 12px/1 var(--font-mono)',color:'var(--text-strong)'}}>{r.k}</span>{r.dirty&&<span title="Non appliquée" style={{width:6,height:6,borderRadius:99,background:'#e5b454',flex:'none'}}></span>}</div>
    <div style={{...cell,display:'flex',alignItems:'center',gap:8}}>
      <span style={{...cell,font:'400 12px/1 var(--font-mono)',color:r.secret?'var(--text-subtle)':'var(--text-body)'}}>{shown}</span>
      {!r.secret&&h&&<button onClick={()=>setReveal(!reveal)} style={{background:'none',border:0,padding:0,cursor:'pointer',color:'var(--text-subtle)',display:'flex',flex:'none'}}><Icon name={reveal?'eye-off':'eye'} size={13}/></button>}
    </div>
    <EnvChips value={r.envs} small/>
    <div style={{display:'flex',flexDirection:'column',gap:5,...cell}}><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-body)'}}>{r.upd}</span><span style={{...lbl,fontSize:11}}>{r.by}</span></div>
    <div style={{display:'flex',gap:2,justifyContent:'flex-end',opacity:h?1:0,transition:'opacity var(--dur-fast)'}}>
      {!r.secret&&<IconButton size={28} variant="ghost" icon="copy" label="Copier" onClick={()=>{navigator.clipboard&&navigator.clipboard.writeText(r.k+'='+r.v);flash('Copié')}}/>}
      <IconButton size={28} variant="ghost" icon="pencil" label="Modifier" onClick={onEdit}/>
      <IconButton size={28} variant="ghost" icon="trash-2" label="Supprimer" onClick={onDelete}/>
    </div>
  </div>;
}

function ProjectVariables({project='ciphera-web'}){
  const [rows,setRows]=React.useState(INIT);
  const [env,setEnv]=React.useState('Production');
  const [q,setQ]=React.useState('');
  const [edit,setEdit]=React.useState(null);
  const [reveal,setReveal]=React.useState({});
  const [raw,setRaw]=React.useState(false);
  const [rawTxt,setRawTxt]=React.useState('');
  const [dirty,setDirty]=React.useState(0);
  const [openG,setOpenG]=React.useState(true);
  const [toast,setToast]=React.useState('');
  const flash=m=>{setToast(m);clearTimeout(flash.t);flash.t=setTimeout(()=>setToast(''),2000)};
  const list=rows.filter(r=>r.envs.includes(env)&&r.k.toLowerCase().includes(q.toLowerCase()));
  const globals=(window.CIPHERA_GLOBALS||[]).filter(g=>(g.proj==='all'||g.proj.includes(project))&&g.envs.includes(env));
  const own=new Set(list.map(r=>r.k));
  const touch=()=>setDirty(n=>n+1);
  const save=(id,data)=>{setRows(rs=>id==='new'?[V({...data,upd:"à l'instant",dirty:true}),...rs]:rs.map(r=>r.id===id?{...r,...data,upd:"à l'instant",dirty:true}:r));setEdit(null);touch()};
  const del=id=>{setRows(rs=>rs.filter(r=>r.id!==id));touch();flash('Variable supprimée')};
  const openRaw=()=>{setRawTxt(list.map(r=>r.k+'='+(r.secret?'':r.v)+(r.secret?'  # secret':'')).join('\n')+'\n');setRaw(true)};
  const applyRaw=()=>{const parsed=rawTxt.split('\n').map(l=>l.trim()).filter(l=>l&&!l.startsWith('#')&&l.includes('=')).map(l=>{const i=l.indexOf('=');const v=l.slice(i+1).replace(/\s+#\s*secret$/,'').trim();return [l.slice(0,i).trim(),v]});
    setRows(rs=>{const keep=rs.filter(r=>!r.envs.includes(env));const out=[...keep];parsed.forEach(([k,v])=>{const old=rs.find(r=>r.k===k&&r.envs.includes(env));out.push(old?{...old,v:old.secret&&!v?old.v:v,dirty:true}:V({k,v,envs:[env],upd:"à l'instant",dirty:true}))});return out});
    setRaw(false);touch();flash(parsed.length+' variables enregistrées')};
  const download=()=>{const a=document.createElement('a');a.href=URL.createObjectURL(new Blob([list.map(r=>r.k+'='+(r.secret?'':r.v)).join('\n')+'\n'],{type:'text/plain'}));a.download=project+'.'+env.toLowerCase()+'.env';document.body.appendChild(a);a.click();a.remove();flash('.env téléchargé')};
  return <div style={{display:'flex',flexDirection:'column',gap:16}}>
    <div style={{display:'flex',alignItems:'center',gap:10,paddingLeft:12}}>
      <Tabs tabs={ENVS.map(e=>({value:e,label:e,count:rows.filter(r=>r.envs.includes(e)).length}))} value={env} onChange={e=>{setEnv(e);setEdit(null);setRaw(false)}}/>
      <span style={{flex:1}}></span>
      <div style={{display:'flex',alignItems:'center',gap:8,height:32,padding:'0 10px',width:220,borderRadius:'var(--radius-md)',border:'1px solid var(--border-strong)',background:'var(--surface-control)'}}><Icon name="search" size={14} color="var(--text-subtle)"/><input value={q} onChange={e=>setQ(e.target.value)} placeholder="Rechercher une clé" style={{flex:1,minWidth:0,background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 12px/1 var(--font-mono)'}}/></div>
      <div style={{display:'flex',gap:2,padding:2,borderRadius:9,border:'1px solid var(--border-default)',background:'var(--surface-control)'}}>
        {[['table','list','Tableau'],['raw','file-code','.env']].map(([k,ic,t])=>{const on=(k==='raw')===raw;return <button key={k} onClick={()=>k==='raw'?openRaw():setRaw(false)} style={{display:'flex',alignItems:'center',gap:6,height:28,padding:'0 10px',border:0,borderRadius:7,cursor:'pointer',background:on?'var(--surface-selected)':'transparent',color:on?'var(--text-strong)':'var(--text-muted)',font:'500 12px/1 var(--font-sans)'}}><Icon name={ic} size={13}/>{t}</button>})}
      </div>
      <IconButton icon="download" label="Télécharger .env" onClick={download}/>
      <Button variant="inverse" icon="plus" onClick={()=>{setRaw(false);setEdit('new')}}>Ajouter</Button>
    </div>
    {dirty>0&&<div style={{display:'flex',alignItems:'center',gap:12,padding:'10px 12px 10px 14px',borderRadius:'var(--radius-lg)',border:'1px solid rgba(229,180,84,.25)',background:'rgba(229,180,84,.06)'}}>
      <Icon name="circle-alert" size={15} color="#e5b454"/>
      <span style={{flex:1,font:'400 12px/1.4 var(--font-sans)',color:'var(--text-body)'}}><span style={{color:'var(--text-strong)',fontWeight:500}}>{dirty} modification{dirty>1?'s':''} non appliquée{dirty>1?'s':''}.</span> Les variables sont injectées au prochain déploiement.</span>
      <Button size="sm" variant="ghost" onClick={()=>{setRows(INIT());setDirty(0)}}>Annuler</Button>
      <Button size="sm" variant="inverse" icon="rocket" onClick={()=>{setRows(rs=>rs.map(r=>({...r,dirty:false})));setDirty(0);flash('Redéploiement lancé avec les nouvelles variables')}}>Enregistrer et redéployer</Button>
    </div>}
    {raw?<div style={{display:'flex',flexDirection:'column',gap:10}}>
      <div style={{borderRadius:'var(--radius-lg)',border:'1px solid var(--border-default)',overflow:'hidden'}}>
        <div style={{display:'flex',alignItems:'center',gap:8,padding:'10px 14px',borderBottom:hair,...lbl}}><Icon name="file-code" size={13}/>{project}.{env.toLowerCase()}.env<span style={{flex:1}}></span><span>Les secrets vides conservent leur valeur actuelle</span></div>
        <textarea value={rawTxt} onChange={e=>setRawTxt(e.target.value)} spellCheck="false" className="log-scroll" style={{display:'block',width:'100%',height:300,resize:'vertical',padding:'14px 16px',border:0,background:'var(--gray-0)',color:'var(--text-body)',font:'400 12px/1.8 var(--font-mono)',outline:'none'}}></textarea>
      </div>
      <div style={{display:'flex',gap:8,justifyContent:'flex-end'}}><Button onClick={()=>setRaw(false)}>Annuler</Button><Button variant="filled" icon="check" onClick={applyRaw}>Enregistrer</Button></div>
    </div>:<div style={{borderTop:hair}}>
      <div style={{display:'grid',gridTemplateColumns:COLS,gap:12,padding:'12px 12px',borderBottom:hair,...lbl}}><span style={{paddingLeft:20}}>Clé</span><span>Valeur</span><span>Environnements</span><span>Modifiée</span><span></span></div>
      {edit==='new'&&<VarRow r={{id:'new',k:'',v:'',secret:false,envs:[env]}} editing onSave={d=>save('new',d)} onCancel={()=>setEdit(null)}/>}
      {list.map(r=><VarRow key={r.id} r={r} editing={edit===r.id} onEdit={()=>setEdit(r.id)} onSave={d=>save(r.id,d)} onCancel={()=>setEdit(null)} onDelete={()=>del(r.id)} reveal={!!reveal[r.id]} setReveal={x=>setReveal(s=>({...s,[r.id]:x}))} flash={flash}/>)}
      {!list.length&&edit!=='new'&&<div style={{padding:'28px 12px',...lbl}}>Aucune variable pour {env}.</div>}
    </div>}
    <div style={{marginTop:12,borderRadius:'var(--radius-lg)',border:'1px solid var(--border-default)',overflow:'hidden'}}>
      <button onClick={()=>setOpenG(o=>!o)} style={{display:'flex',alignItems:'center',gap:10,width:'100%',padding:'14px 16px',background:'transparent',border:0,cursor:'pointer',textAlign:'left'}}>
        <Icon name={openG?'chevron-down':'chevron-right'} size={14} color="var(--text-muted)"/>
        <Icon name="braces" size={14} color="var(--text-muted)"/>
        <span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Héritées des variables globales</span>
        <span style={{...lbl,fontFamily:'var(--font-mono)'}}>{globals.length}</span>
        <span style={{flex:1}}></span><span style={lbl}>Plateforme › Variables globales</span>
      </button>
      {openG&&<div style={{borderTop:hair}}>
        {globals.map((g,i)=>{const over=own.has(g.k);return <div key={i} style={{display:'grid',gridTemplateColumns:'minmax(0,1.2fr) minmax(0,1.6fr) minmax(0,1fr)',gap:12,alignItems:'center',minHeight:44,padding:'0 16px 0 40px',borderTop:i?hair:'none',opacity:over?.5:1}}>
          <span style={{...cell,display:'flex',alignItems:'center',gap:8,font:'500 12px/1 var(--font-mono)',color:'var(--text-body)',textDecoration:over?'line-through':'none'}}>{g.secret&&<Icon name="lock" size={11} color="var(--text-subtle)"/>}{g.k}</span>
          <span style={{...cell,font:'400 12px/1 var(--font-mono)',color:'var(--text-subtle)'}}>{g.secret?'••••••••••••':g.v}</span>
          <span style={{justifySelf:'end'}}>{over?<Badge>Remplacée par le projet</Badge>:<Badge>Globale</Badge>}</span>
        </div>})}
        {!globals.length&&<div style={{padding:'16px 40px',...lbl}}>Aucune variable globale pour {env}.</div>}
      </div>}
    </div>
    {toast&&<div style={{position:'fixed',bottom:24,right:24,zIndex:60,padding:'10px 14px',borderRadius:'var(--radius-md)',background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',font:'400 12px/1 var(--font-sans)',color:'var(--text-strong)',display:'flex',alignItems:'center',gap:8}}><Icon name="check" size={12} color="var(--green-500)"/>{toast}</div>}
  </div>;
}
window.ProjectVariables=ProjectVariables;
})();
