(()=>{
const {Button,IconButton,Badge,Icon,Modal,Switch}=window.CipheraDesignSystem;
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const card={borderRadius:'var(--radius-xl)',border:'1px solid var(--border-default)',background:'var(--surface-app)'};
const hair='1px solid var(--border-subtle)';
const KEY='ciphera.backup-target';
const SUPA='../../web/src/assets/images/connectors/supabase-mark.png';
const mono=(v,c='var(--text-body)',s=12)=><span style={{fontFamily:'var(--font-mono)',fontSize:s,color:c}}>{v}</span>;
const pill=(t,warn)=><span style={{display:'inline-flex',alignItems:'center',height:22,padding:'0 8px',borderRadius:6,background:warn?'rgba(229,180,84,.12)':'var(--surface-selected)',font:'500 12px/1 var(--font-mono)',color:warn?'#e5b454':'var(--text-body)'}}>{t}</span>;
const INIT=[
  {id:1,kind:'auto',name:'Sauvegarde quotidienne',when:"aujourd'hui · 03:00",size:'4,2 Go',st:'ok'},
  {id:2,kind:'auto',name:'Sauvegarde quotidienne',when:'hier · 03:00',size:'4,1 Go',st:'ok'},
  {id:3,kind:'snap',name:'Avant mise à jour Node 22',when:'il y a 3 j',size:'12,8 Go',st:'ok'},
  {id:4,kind:'auto',name:'Sauvegarde quotidienne',when:'il y a 3 j · 03:00',size:'4,1 Go',st:'ok'},
  {id:5,kind:'auto',name:'Sauvegarde quotidienne',when:'il y a 4 j · 03:00',size:'4,0 Go',st:'error'},
  {id:6,kind:'auto',name:'Sauvegarde quotidienne',when:'il y a 5 j · 03:00',size:'4,0 Go',st:'ok'},
  {id:7,kind:'snap',name:'Installation initiale',when:'il y a 41 j',size:'9,6 Go',st:'ok'},
];
function Target({onSave,onClose,cur}){
  const supaOn=(()=>{try{return !!JSON.parse(localStorage.getItem('ciphera.connectors')||'{}').supabase}catch(e){return false}})();
  const [t,setT]=React.useState(cur?.type||'supabase');const [s3,setS3]=React.useState(cur?.s3||{endpoint:'https://s3.fr-par.scw.cloud',bucket:'',region:'fr-par',key:'',secret:''});const [test,setTest]=React.useState('idle');
  const f=(k,l,ph,type='text')=><label style={{display:'flex',flexDirection:'column',gap:7}}><span style={{font:'500 12px/1 var(--font-sans)',color:'var(--text-strong)'}}>{l}</span><input type={type} value={s3[k]} onChange={e=>{setS3({...s3,[k]:e.target.value});setTest('idle')}} placeholder={ph} style={{height:38,padding:'0 11px',borderRadius:9,border:'1px solid var(--border-default)',background:'var(--surface-raised)',color:'var(--text-strong)',font:'400 13px/1 var(--font-mono)',outline:'none'}}/></label>;
  const s3ok=s3.endpoint&&s3.bucket&&s3.key&&s3.secret;
  const opt=(k,img,ic,title,d)=>{const on=t===k;return <button onClick={()=>setT(k)} style={{display:'flex',alignItems:'center',gap:12,padding:'14px',borderRadius:12,cursor:'pointer',textAlign:'left',border:'1px solid '+(on?'var(--border-strong)':'var(--border-subtle)'),background:on?'var(--surface-raised)':'transparent'}}><span style={{width:38,height:38,borderRadius:10,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}>{img?<img src={img} alt="" style={{width:20,height:20,objectFit:'contain'}}/>:<Icon name={ic} size={17} color="var(--text-strong)"/>}</span><span style={{display:'flex',flexDirection:'column',gap:6,flex:1}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{title}</span><span style={{font:'400 12px/1.3 var(--font-sans)',color:'var(--text-subtle)'}}>{d}</span></span><span style={{width:18,height:18,borderRadius:99,border:'2px solid '+(on?'#8b7cf6':'var(--border-strong)'),display:'flex',alignItems:'center',justifyContent:'center'}}>{on&&<span style={{width:8,height:8,borderRadius:99,background:'#8b7cf6'}}></span>}</span></button>};
  const ok=t==='supabase'?supaOn:s3ok&&test==='ok';
  return <Modal title="Destination des sauvegardes" description="Les sauvegardes automatiques sont chiffrées puis envoyées vers ce stockage." onClose={onClose} width={560}
    footer={<><Button onClick={onClose}>Annuler</Button><Button variant="inverse" disabled={!ok} onClick={()=>onSave(t==='supabase'?{type:'supabase',label:'Supabase Storage',dest:'backups/srv-par-01'}:{type:'s3',label:'S3 · '+s3.bucket,dest:s3.endpoint.replace(/^https?:\/\//,'')+'/'+s3.bucket,s3})}>Enregistrer</Button></>}>
    <div style={{display:'flex',flexDirection:'column',gap:16}}>
      <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:8}}>{opt('supabase',SUPA,null,'Supabase','Via le connecteur Supabase')}{opt('s3',null,'database','Stockage S3','AWS, Scaleway, R2, MinIO…')}</div>
      {t==='supabase'&&(supaOn?<div style={{display:'flex',alignItems:'center',gap:10,padding:'12px 14px',borderRadius:12,background:'var(--surface-raised)',font:'400 13px/1.4 var(--font-sans)',color:'var(--text-muted)'}}><Icon name="circle-check" size={15} color="var(--green-500)"/>Connecteur Supabase actif. Les fichiers seront stockés dans le bucket <span style={{fontFamily:'var(--font-mono)',color:'var(--text-strong)'}}>backups</span>.</div>
        :<div style={{display:'flex',alignItems:'center',gap:12,padding:'12px 14px',borderRadius:12,border:'1px solid rgba(229,180,84,.25)',background:'rgba(229,180,84,.06)'}}><Icon name="plug" size={15} color="#e5b454"/><span style={{flex:1,font:'400 13px/1.4 var(--font-sans)',color:'var(--text-body)'}}>Le connecteur Supabase n'est pas configuré.</span><Button size="sm" onClick={()=>{try{const c=JSON.parse(localStorage.getItem('ciphera.connectors')||'{}');c.supabase={url:'https://xyz.supabase.co'};localStorage.setItem('ciphera.connectors',JSON.stringify(c))}catch(e){};setT('s3');setTimeout(()=>setT('supabase'),0)}}>Connecter Supabase</Button></div>)}
      {t==='s3'&&<div style={{display:'flex',flexDirection:'column',gap:12}}>
        <div style={{display:'grid',gridTemplateColumns:'1.6fr 1fr',gap:10}}>{f('endpoint','Endpoint','https://s3.amazonaws.com')}{f('region','Région','eu-west-3')}</div>
        {f('bucket','Bucket','ciphera-backups')}
        <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:10}}>{f('key','Access key','AKIA…')}{f('secret','Secret key','••••••••','password')}</div>
        <div style={{display:'flex',alignItems:'center',gap:10}}><Button size="sm" icon={test==='testing'?'loader':'plug-zap'} disabled={!s3ok||test==='testing'} onClick={()=>{setTest('testing');setTimeout(()=>setTest('ok'),1100)}}>Tester la connexion</Button>{test==='ok'&&<span style={{display:'flex',alignItems:'center',gap:6,font:'400 12px/1 var(--font-sans)',color:'var(--green-500)'}}><Icon name="circle-check" size={13}/>Bucket accessible en écriture</span>}</div>
      </div>}
    </div>
  </Modal>;
}
function BackupsView({s,flash,back}){
  const [target,setTargetS]=React.useState(()=>{try{return JSON.parse(localStorage.getItem(KEY))}catch(e){return null}});
  const setTarget=v=>{setTargetS(v);try{v?localStorage.setItem(KEY,JSON.stringify(v)):localStorage.removeItem(KEY)}catch(e){}};
  const [list,setList]=React.useState(INIT);const [modal,setModal]=React.useState(false);const [auto,setAuto]=React.useState({on:true,freq:'Quotidienne',hour:'03:00',keep:'7 jours'});const [restore,setRestore]=React.useState(null);const [running,setRunning]=React.useState(null);
  const [tab,setTab]=React.useState('all');
  const runNow=kind=>{if(!target&&kind==='auto'){setModal(true);return}const id=Date.now();setRunning(id);setList(l=>[{id,kind,name:kind==='snap'?'Snapshot manuel':'Sauvegarde manuelle',when:"à l'instant",size:'…',st:'pending'},...l]);setTimeout(()=>{setList(l=>l.map(x=>x.id===id?{...x,st:'ok',size:kind==='snap'?'12,9 Go':'4,2 Go'}:x));setRunning(null);flash(kind==='snap'?'Snapshot créé':'Sauvegarde envoyée vers '+target.label)},2200)};
  const rows=list.filter(x=>tab==='all'||x.kind===tab);
  const sel=(v,opts,k)=><span style={{position:'relative',display:'inline-flex',alignItems:'center'}}><select value={v} onChange={e=>setAuto({...auto,[k]:e.target.value})} style={{height:32,padding:'0 30px 0 10px',borderRadius:8,border:'1px solid var(--border-default)',background:'var(--surface-raised)',color:'var(--text-strong)',font:'500 12px/1 var(--font-sans)',outline:'none',appearance:'none',WebkitAppearance:'none',cursor:'pointer'}}>{opts.map(o=><option key={o} style={{background:'#141417'}}>{o}</option>)}</select><Icon name="chevron-down" size={13} color="var(--text-muted)" style={{position:'absolute',right:9,pointerEvents:'none'}}/></span>;
  const sep=<span style={{width:1,height:14,background:'var(--border-strong)'}}></span>;
  return <div style={{display:'flex',flexDirection:'column',gap:14,paddingTop:4}}>
    <button onClick={back} style={{display:'flex',alignItems:'center',gap:6,alignSelf:'flex-start',padding:0,border:0,background:'transparent',cursor:'pointer',font:'400 13px/1 var(--font-sans)',color:'var(--text-muted)'}}><Icon name="arrow-left" size={14}/>{s.name}</button>
    <div style={{...card,borderRadius:20,padding:'22px 24px',display:'flex',flexDirection:'column',gap:18}}>
      <div style={{display:'flex',alignItems:'center',gap:16}}>
        <span style={{width:52,height:52,borderRadius:13,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'linear-gradient(145deg,#4a4a52,#26262c)',boxShadow:'inset 0 1px 0 rgba(255,255,255,.18)'}}><Icon name="archive" size={24} color="#f2f2f2"/></span>
        <span style={{display:'flex',flexDirection:'column',gap:8,flex:1,minWidth:0}}><span style={{display:'flex',alignItems:'center',gap:10}}><span style={{font:'600 18px/1 var(--font-sans)',color:'var(--text-strong)'}}>Snapshots et sauvegardes</span><span style={{height:22,display:'flex',alignItems:'center',padding:'0 8px',borderRadius:6,background:target&&auto.on?'rgba(34,197,94,.12)':'rgba(229,180,84,.12)',font:'500 12px/1 var(--font-sans)',color:target&&auto.on?'var(--green-500)':'#e5b454'}}>{!target?'Destination à configurer':auto.on?'Automatiques':'En pause'}</span></span><span style={lbl}>{s.name} · {list.length} points de restauration</span></span>
        <div style={{display:'flex',borderRadius:10,overflow:'hidden'}}><button onClick={()=>runNow('auto')} disabled={!!running} style={{display:'flex',alignItems:'center',gap:7,height:38,padding:'0 16px',border:0,cursor:running?'default':'pointer',background:'#8b7cf6',color:'#fff',font:'600 13px/1 var(--font-sans)',opacity:running?.6:1}}><Icon name={running?'loader':'upload-cloud'} size={14} color="#fff" style={running?{animation:'spin 1s linear infinite'}:undefined}/>Sauvegarder maintenant</button></div>
        <Button icon="camera" disabled={!!running} onClick={()=>runNow('snap')}>Snapshot</Button>
      </div>
      <div style={{display:'flex',alignItems:'center',gap:14,padding:'14px 18px',borderRadius:12,background:'var(--surface-raised)'}}>
        {target?<>
          <span style={{width:34,height:34,borderRadius:9,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}>{target.type==='supabase'?<img src={SUPA} alt="" style={{width:18,height:18,objectFit:'contain'}}/>:<Icon name="database" size={16} color="var(--text-strong)"/>}</span>
          <span style={{display:'flex',flexDirection:'column',gap:6,flex:1,minWidth:0}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{target.label}</span><span style={{font:'400 12px/1 var(--font-mono)',color:'var(--text-subtle)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{target.dest}</span></span>
          <button onClick={()=>setModal(true)} style={{padding:0,border:0,background:'transparent',cursor:'pointer',font:'500 13px/1 var(--font-sans)',color:'#a78bfa'}}>Modifier</button>
        </>:<>
          <Icon name="hard-drive-upload" size={16} color="#e5b454"/>
          <span style={{flex:1,font:'400 13px/1.4 var(--font-sans)',color:'var(--text-body)'}}>Choisissez où envoyer les sauvegardes : connecteur Supabase ou stockage S3.</span>
          <Button size="sm" variant="inverse" onClick={()=>setModal(true)}>Configurer</Button>
        </>}
      </div>
      <div style={{display:'flex',alignItems:'center',gap:16,flexWrap:'wrap',opacity:target?1:.5,pointerEvents:target?'auto':'none'}}>
        <span style={{display:'flex',alignItems:'center',gap:10}}><Switch checked={auto.on} onChange={v=>{setAuto({...auto,on:v});flash(v?'Sauvegardes automatiques activées':'Sauvegardes automatiques en pause')}}/><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Sauvegardes automatiques</span></span>
        {sep}<span style={{display:'flex',alignItems:'center',gap:8,...lbl}}>Fréquence {sel(auto.freq,['Toutes les 6 h','Quotidienne','Hebdomadaire'],'freq')}</span>
        <span style={{display:'flex',alignItems:'center',gap:8,...lbl}}>à {sel(auto.hour,['00:00','03:00','05:00','23:00'],'hour')}</span>
        {sep}<span style={{display:'flex',alignItems:'center',gap:8,...lbl}}>Conserver {sel(auto.keep,['7 jours','14 jours','30 jours','90 jours'],'keep')}</span>
        <span style={{flex:1}}></span>
        <span style={lbl}>Prochaine : {auto.on&&target?'demain · '+auto.hour:'—'}</span>
      </div>
    </div>
    <div style={{display:'flex',alignItems:'center',gap:4,paddingLeft:12,marginTop:6}}>{[['all','Tous',list.length],['auto','Sauvegardes',list.filter(x=>x.kind==='auto').length],['snap','Snapshots',list.filter(x=>x.kind==='snap').length]].map(([k,t,n])=>{const on=tab===k;return <button key={k} onClick={()=>setTab(k)} style={{display:'flex',alignItems:'center',gap:8,height:34,padding:'0 12px',border:0,borderRadius:8,cursor:'pointer',background:on?'var(--surface-selected)':'transparent',font:'500 13px/1 var(--font-sans)',color:on?'var(--text-strong)':'var(--text-muted)'}}>{t}<span style={{font:'500 11px/1 var(--font-mono)',color:'var(--text-subtle)'}}>{n}</span></button>})}</div>
    <div style={card}>
      <div style={{display:'grid',gridTemplateColumns:'minmax(0,1.6fr) minmax(0,1fr) 100px 110px 170px',gap:16,padding:'14px 16px',font:'500 12px/1 var(--font-sans)',color:'var(--text-muted)'}}><span>Point de restauration</span><span>Date</span><span>Taille</span><span>Statut</span><span></span></div>
      {rows.map(b=><div key={b.id} style={{display:'grid',gridTemplateColumns:'minmax(0,1.6fr) minmax(0,1fr) 100px 110px 170px',gap:16,alignItems:'center',padding:'13px 16px',borderTop:hair}}>
        <span style={{display:'flex',alignItems:'center',gap:12,minWidth:0}}><span style={{width:40,height:40,borderRadius:10,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'var(--surface-selected)'}}><Icon name={b.kind==='snap'?'camera':'archive'} size={17} color="var(--text-strong)"/></span><span style={{display:'flex',flexDirection:'column',gap:7,minWidth:0}}><span style={{font:'500 13px/1.1 var(--font-sans)',color:'var(--text-strong)',whiteSpace:'nowrap',overflow:'hidden',textOverflow:'ellipsis'}}>{b.name}</span><span style={lbl}>{b.kind==='snap'?'Snapshot · disque complet':'Sauvegarde · '+(target?target.label:'local')}</span></span></span>
        <span>{pill(b.when)}</span>
        {mono(b.size,'var(--text-body)')}
        <span style={{display:'flex',alignItems:'center',gap:7}}><Icon name={b.st==='ok'?'circle-check':b.st==='error'?'circle-x':'loader'} size={15} color={b.st==='ok'?'var(--green-500)':b.st==='error'?'#f87171':'var(--text-muted)'} style={b.st==='pending'?{animation:'spin 1s linear infinite'}:undefined}/><span style={{font:'400 13px/1 var(--font-sans)',color:'var(--text-body)'}}>{b.st==='ok'?'Terminé':b.st==='error'?'Échec':'En cours'}</span></span>
        <span style={{display:'flex',gap:6,justifyContent:'flex-end'}}>{b.st==='ok'&&<Button size="sm" onClick={()=>setRestore(b)}>Restaurer</Button>}{b.st==='error'&&<Button size="sm" onClick={()=>runNow('auto')}>Relancer</Button>}<IconButton size={28} icon="trash-2" label="Supprimer" onClick={()=>{setList(l=>l.filter(x=>x.id!==b.id));flash('Point de restauration supprimé')}}/></span>
      </div>)}
    </div>
    {modal&&<Target cur={target} onClose={()=>setModal(false)} onSave={v=>{setTarget(v);setModal(false);flash('Destination enregistrée · '+v.label)}}/>}
    {restore&&<Modal title="Restaurer ce point ?" description={'Le serveur '+s.name+' sera arrêté puis restauré à l\'état du '+restore.when+'. Les modifications postérieures seront perdues.'} onClose={()=>setRestore(null)} width={480} footer={<><Button onClick={()=>setRestore(null)}>Annuler</Button><Button variant="inverse" icon="history" onClick={()=>{setRestore(null);flash('Restauration de '+s.name+' lancée')}}>Restaurer</Button></>}><div style={{display:'flex',alignItems:'center',gap:12,padding:'12px 14px',borderRadius:12,background:'var(--surface-raised)'}}><Icon name={restore.kind==='snap'?'camera':'archive'} size={16} color="var(--text-muted)"/><span style={{display:'flex',flexDirection:'column',gap:6}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{restore.name}</span><span style={lbl}>{restore.when} · {restore.size}</span></span></div></Modal>}
  </div>;
}
window.BackupsView=BackupsView;
})();
