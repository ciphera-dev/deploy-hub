(()=>{
const {Button,IconButton,Icon}=window.CipheraDesignSystem;
const lbl={font:'400 12px/1 var(--font-sans)',color:'var(--text-subtle)'};
const MAX=5;
const MODELS={openai:['GPT-4.1','GPT-4.1 mini','o4-mini'],claude:['Claude Sonnet 4.5','Claude Opus 4.1','Claude Haiku 4.5'],groq:['Llama 3.3 70B','Qwen 3 32B','Llama 3.1 8B Instant'],openrouter:['DeepSeek V3','Gemini 2.5 Pro','Mistral Large']};
const KEY='ciphera.connectors.on';
function useLLMs(){
  const read=()=>{const lib=window.CIPHERA_LLM||[];let on;try{on=JSON.parse(localStorage.getItem(KEY))}catch(e){}if(!Array.isArray(on))on=lib.filter(c=>c.on).map(c=>c.id);return lib.filter(c=>on.includes(c.id))};
  const [v,setV]=React.useState(read);
  React.useEffect(()=>{const f=()=>setV(read());window.addEventListener('ciphera:connectors',f);window.addEventListener('storage',f);return()=>{window.removeEventListener('ciphera:connectors',f);window.removeEventListener('storage',f)}},[]);
  return v;
}
const guessType=s=>/panne|down|injoignable|incident|hors ligne|500|indisponible/i.test(s)?'incident':/id[ée]e|sugg|ajouter|serait bien|am[ée]lior|pouvoir|proposer/i.test(s)?'idea':/bug|erreur|crash|plante|marche pas|fonctionne pas|bloqu|fig[ée]|cass/i.test(s)?'bug':/\?|comment|possible/i.test(s)?'question':'bug';
const guessPrio=(s,t)=>t==='incident'||/urgent|production|clients?|bloquant/i.test(s)?'high':t==='idea'?'low':'med';
const cap=s=>s.charAt(0).toUpperCase()+s.slice(1);
function makeDraft(msgs,files,TK){
  const all=msgs.join(' ');const type=guessType(all);const proj=TK.PROJECTS.find(p=>all.toLowerCase().includes(p.toLowerCase()))||'Plateforme';
  let first=msgs[0].split(/[.!?\n]/)[0].trim().replace(/^(bonjour|salut|hello)[, ]*/i,'');if(first.length>72)first=first.slice(0,69).replace(/\s+\S*$/,'')+'…';
  const title=cap(first||'Nouveau ticket');
  const ctx=msgs[0].trim();const extra=msgs.slice(1).map(m=>'• '+m.trim()).join('\n');
  const desc=type==='idea'?'Besoin\n'+ctx+(extra?'\n\nPrécisions\n'+extra:'')+'\n\nBénéfice attendu\nAméliorer l\'expérience de l\'équipe et des clients sur '+proj+'.'
    :type==='question'?'Question\n'+ctx+(extra?'\n\nContexte\n'+extra:'')
    :(type==='incident'?'Impact\n':'Comportement observé\n')+ctx+(extra?'\n\nDétails\n'+extra:'')+'\n\nComportement attendu\nLe service fonctionne normalement sur '+proj+'.';
  return {title,type,prio:guessPrio(all,type),project:proj,desc:desc+(files.length?'\n\nPièces jointes\n'+files.map(f=>'• '+f.name).join('\n'):''),labels:[type==='idea'?'roadmap':type==='incident'?'infra':'à trier']};
}
const ASK={bug:'Depuis quand le problème apparaît-il, et sur quel navigateur ou environnement ?',idea:'Qui en bénéficierait en priorité, et à quelle fréquence l\'utiliseraient-ils ?',question:'Qu\'avez-vous déjà essayé, et sur quel projet ?',incident:'Combien d\'utilisateurs sont touchés, et depuis quelle heure ?'};

function ModelPicker({llms,prov,setProv,model,setModel}){
  const [open,setOpen]=React.useState(false);const ref=React.useRef();
  React.useEffect(()=>{if(!open)return;const f=e=>{if(ref.current&&!ref.current.contains(e.target))setOpen(false)};document.addEventListener('mousedown',f);return()=>document.removeEventListener('mousedown',f)},[open]);
  const P=llms.find(l=>l.id===prov)||llms[0];
  return <div ref={ref} style={{position:'relative'}}>
    <button onClick={()=>setOpen(o=>!o)} style={{display:'flex',alignItems:'center',gap:7,height:30,padding:'0 10px 0 8px',borderRadius:99,border:'1px solid rgba(255,255,255,.1)',background:open?'rgba(255,255,255,.06)':'transparent',cursor:'pointer',font:'500 12px/1 var(--font-sans)',color:'var(--text-body)'}}>
      {P.img?<img src={P.img} alt="" style={{width:14,height:14,objectFit:'contain'}}/>:<Icon name={P.ic} size={13}/>}{model}<Icon name="chevron-down" size={12} color="var(--text-subtle)"/></button>
    {open&&<div style={{position:'absolute',bottom:38,left:0,zIndex:40,width:420,display:'grid',gridTemplateColumns:'150px 1fr',borderRadius:14,background:'var(--surface-tooltip)',boxShadow:'var(--shadow-tooltip)',overflow:'hidden'}}>
      <div style={{padding:6,borderRight:'1px solid rgba(255,255,255,.08)',display:'flex',flexDirection:'column',gap:2}}>
        <span style={{padding:'6px 8px 8px',...lbl,fontSize:10,letterSpacing:'.06em',textTransform:'uppercase'}}>Fournisseur</span>
        {llms.map(l=>{const on=l.id===P.id;return <button key={l.id} onClick={()=>{setProv(l.id);setModel(MODELS[l.id][0])}} style={{display:'flex',alignItems:'center',gap:9,height:34,padding:'0 8px',borderRadius:8,border:0,cursor:'pointer',background:on?'rgba(255,255,255,.08)':'transparent',font:'500 13px/1 var(--font-sans)',color:on?'var(--text-strong)':'var(--text-body)',textAlign:'left'}}>{l.img?<img src={l.img} alt="" style={{width:15,height:15,objectFit:'contain'}}/>:<Icon name={l.ic} size={14}/>}{l.n}</button>})}
      </div>
      <div style={{padding:6,display:'flex',flexDirection:'column',gap:2}}>
        <span style={{padding:'6px 8px 8px',...lbl,fontSize:10,letterSpacing:'.06em',textTransform:'uppercase'}}>Modèle</span>
        {MODELS[P.id].map((m,i)=>{const on=m===model;return <button key={m} onClick={()=>{setModel(m);setOpen(false)}} style={{display:'flex',alignItems:'center',gap:8,height:40,padding:'0 10px',borderRadius:8,border:0,cursor:'pointer',background:on?'rgba(255,255,255,.08)':'transparent',textAlign:'left'}}><span style={{display:'flex',flexDirection:'column',gap:4,flex:1}}><span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>{m}</span><span style={{...lbl,fontSize:11}}>{['Équilibré','Le plus capable','Le plus rapide'][i]}</span></span>{on&&<Icon name="check" size={13}/>}</button>})}
      </div>
    </div>}
  </div>;
}

function TicketAICompose({onCreate,onNav}){
  const TK=window.__TK;const llms=useLLMs();
  const [prov,setProv]=React.useState(null);const [model,setModel]=React.useState(null);
  React.useEffect(()=>{if(llms.length&&!llms.find(l=>l.id===prov)){setProv(llms[0].id);setModel(MODELS[llms[0].id][0])}},[llms]);
  const [msgs,setMsgs]=React.useState([]);const [files,setFiles]=React.useState([]);const [pending,setPending]=React.useState([]);const [v,setV]=React.useState('');const [busy,setBusy]=React.useState(false);const [draft,setDraft]=React.useState(null);const [drag,setDrag]=React.useState(false);
  const fileRef=React.useRef();const scRef=React.useRef();
  const used=msgs.filter(m=>m.role==='user').length;const left=MAX-used;
  React.useEffect(()=>{if(scRef.current)scRef.current.scrollTop=scRef.current.scrollHeight},[msgs.length,busy,draft]);
  const addFiles=list=>{[...list].forEach(f=>{const it={name:f.name,size:f.size,type:f.type,url:null};if(f.type.startsWith('image/')){const r=new FileReader();r.onload=()=>setPending(p=>p.map(x=>x===it?{...x,url:r.result}:x));r.readAsDataURL(f)}setPending(p=>[...p,it])})};
  const send=()=>{if(busy||left<=0||(!v.trim()&&!pending.length))return;const text=v.trim()||'Voir les pièces jointes.';const nf=[...files,...pending];const um={role:'user',text,files:pending};const next=[...msgs,um];setMsgs(next);setFiles(nf);setPending([]);setV('');setBusy(true);
    setTimeout(()=>{const userTexts=next.filter(m=>m.role==='user').map(m=>m.text);const d=makeDraft(userTexts,nf,TK);setDraft(d);const n=userTexts.length;const T=TK.TYPES[d.type][0].toLowerCase();
      const reply=n===1?'J\'ai analysé votre description'+(nf.length?' et '+nf.length+' pièce'+(nf.length>1?'s':'')+' jointe'+(nf.length>1?'s':''):'')+'. Il s\'agit probablement d\'un '+(d.type==='idea'?'suggestion':T)+' sur '+d.project+'. J\'ai préparé un brouillon ci-dessous. Pour le compléter : '+ASK[d.type]
        :n>=MAX?'Merci, le brouillon est à jour. Vous avez atteint la limite de '+MAX+' messages : vérifiez-le puis validez.':'Merci, j\'ai mis à jour le brouillon'+(pending.length?' avec les nouvelles pièces jointes':'')+'. Autre chose à ajouter, ou on valide ?';
      setMsgs(m=>[...m,{role:'ai',text:reply}]);setBusy(false)},1300)};
  const reset=()=>{setMsgs([]);setFiles([]);setPending([]);setDraft(null);setV('')};
  const P=llms.find(l=>l.id===prov);
  if(!llms.length)return <div style={{flex:1,display:'flex',alignItems:'center',justifyContent:'center',padding:24}}>
    <div style={{display:'flex',flexDirection:'column',alignItems:'center',textAlign:'center',gap:12,maxWidth:420}}>
      <span style={{width:48,height:48,borderRadius:14,display:'flex',alignItems:'center',justifyContent:'center',background:'rgba(255,255,255,.05)',border:'1px solid rgba(255,255,255,.1)'}}><Icon name="plug-zap" size={20} color="var(--text-body)"/></span>
      <span style={{font:'600 17px/1.2 var(--font-sans)',color:'var(--text-strong)'}}>Connectez un fournisseur d'IA</span>
      <span style={{font:'400 13px/1.55 var(--font-sans)',color:'var(--text-muted)'}}>L'assistant de tickets utilise vos connecteurs ChatGPT, Claude, Groq ou OpenRouter. Aucun n'est connecté pour le moment.</span>
      <Button variant="inverse" icon="plug" onClick={()=>onNav&&onNav('Connecteurs')}>Ouvrir les connecteurs</Button>
    </div>
  </div>;
  const PRE=TK.PRIO;
  return <div onDragOver={e=>{e.preventDefault();setDrag(true)}} onDragLeave={e=>{if(e.currentTarget===e.target)setDrag(false)}} onDrop={e=>{e.preventDefault();setDrag(false);addFiles(e.dataTransfer.files)}} style={{position:'relative',flex:1,display:'flex',flexDirection:'column',minHeight:0}}>
    {drag&&<div style={{position:'absolute',inset:12,zIndex:30,borderRadius:16,border:'2px dashed rgba(255,255,255,.3)',background:'rgba(12,12,15,.85)',display:'flex',flexDirection:'column',alignItems:'center',justifyContent:'center',gap:10,pointerEvents:'none'}}><Icon name="upload" size={22} color="var(--text-strong)"/><span style={{font:'500 14px/1 var(--font-sans)',color:'var(--text-strong)'}}>Déposez vos fichiers</span><span style={lbl}>Captures, logs, documents</span></div>}
    {msgs.length>0&&<div style={{display:'flex',alignItems:'center',gap:10,padding:'12px 24px',borderBottom:'1px solid rgba(255,255,255,.06)'}}>
      <span style={{font:'500 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Nouveau ticket</span><span style={{...lbl,fontSize:11}}>avec {model}</span><span style={{flex:1}}></span>
      <span style={{display:'flex',alignItems:'center',gap:6,...lbl,fontSize:11}}>{Array.from({length:MAX},(_,i)=><span key={i} style={{width:14,height:4,borderRadius:99,background:i<used?'#ededed':'rgba(255,255,255,.12)'}}></span>)}{used}/{MAX}</span>
      <Button size="sm" variant="ghost" icon="rotate-ccw" onClick={reset}>Recommencer</Button>
    </div>}
    <div ref={scRef} className="log-scroll" style={{flex:1,minHeight:0,overflow:'auto'}}>
      {!msgs.length?<div style={{height:'100%',display:'flex',flexDirection:'column',alignItems:'center',justifyContent:'center',gap:14,padding:24,textAlign:'center'}}>
        <img src="../../web/src/assets/brand/ciphera-icon.svg" alt="" style={{width:42,opacity:.9}}/>
        <span style={{font:'500 26px/1.2 var(--font-sans)',color:'var(--text-strong)',letterSpacing:'-0.01em'}}>Décrivez votre ticket</span>
        <span style={{maxWidth:440,font:'400 13px/1.55 var(--font-sans)',color:'var(--text-muted)'}}>Expliquez le bug, l'idée ou la question avec vos mots et joignez des captures. L'IA pose les bonnes questions, puis crée le titre, la description et la catégorie.</span>
        <div style={{display:'flex',gap:8,marginTop:6,flexWrap:'wrap',justifyContent:'center'}}>{[['image','Captures'],['file-text','Logs et documents'],['message-square','5 échanges max.']].map(([ic,t])=><span key={t} style={{display:'flex',alignItems:'center',gap:6,height:28,padding:'0 11px',borderRadius:99,background:'rgba(255,255,255,.04)',...lbl}}><Icon name={ic} size={12}/>{t}</span>)}</div>
      </div>
      :<div style={{maxWidth:700,margin:'0 auto',padding:'26px 24px',display:'flex',flexDirection:'column',gap:18}}>
        {msgs.map((m,i)=>m.role==='user'?<div key={i} style={{alignSelf:'flex-end',maxWidth:'80%',display:'flex',flexDirection:'column',gap:8,alignItems:'flex-end'}}>
          {m.files.length>0&&<div style={{display:'flex',gap:6,flexWrap:'wrap',justifyContent:'flex-end'}}>{m.files.map((f,j)=>f.url?<img key={j} src={f.url} alt={f.name} style={{width:120,height:80,objectFit:'cover',borderRadius:10,border:'1px solid rgba(255,255,255,.1)'}}/>:<span key={j} style={{display:'flex',alignItems:'center',gap:6,height:30,padding:'0 10px',borderRadius:9,background:'rgba(255,255,255,.05)',font:'400 12px/1 var(--font-sans)',color:'var(--text-body)'}}><Icon name="file-text" size={12}/>{f.name}</span>)}</div>}
          <div style={{padding:'10px 14px',borderRadius:'16px 16px 4px 16px',background:'rgba(255,255,255,.08)',font:'400 14px/1.55 var(--font-sans)',color:'var(--text-strong)'}}>{m.text}</div></div>
          :<div key={i} style={{display:'flex',gap:10,maxWidth:'88%'}}><span style={{width:28,height:28,borderRadius:99,flex:'none',display:'flex',alignItems:'center',justifyContent:'center',background:'#16161a',border:'1px solid rgba(255,255,255,.1)'}}>{P&&P.img?<img src={P.img} alt="" style={{width:14,height:14,objectFit:'contain'}}/>:<Icon name="sparkles" size={13}/>}</span><div style={{display:'flex',flexDirection:'column',gap:6}}><span style={{...lbl,fontSize:11}}>{model}</span><span style={{font:'400 14px/1.6 var(--font-sans)',color:'var(--text-body)'}}>{m.text}</span></div></div>)}
        {busy&&<div style={{display:'flex',alignItems:'center',gap:10,...lbl}}><Icon name="loader" size={13} style={{animation:'spin 1s linear infinite'}}/>Analyse en cours{files.length?' des pièces jointes':''}…</div>}
        {draft&&!busy&&<div style={{borderRadius:16,border:'1px solid rgba(255,255,255,.1)',background:'rgba(255,255,255,.025)',overflow:'hidden'}}>
          <div style={{display:'flex',alignItems:'center',gap:8,padding:'12px 16px',borderBottom:'1px solid rgba(255,255,255,.06)'}}><Icon name="file-pen-line" size={14} color="var(--text-muted)"/><span style={{font:'600 13px/1 var(--font-sans)',color:'var(--text-strong)'}}>Brouillon généré</span><span style={{flex:1}}></span><span style={{...lbl,fontSize:11}}>Vous pourrez le modifier ensuite</span></div>
          <div style={{display:'flex',flexDirection:'column',gap:12,padding:'14px 16px'}}>
            <span style={{font:'600 16px/1.35 var(--font-sans)',color:'var(--text-strong)'}}>{draft.title}</span>
            <div style={{display:'flex',gap:6,flexWrap:'wrap'}}>
              {Object.keys(TK.TYPES).map(k=>{const on=draft.type===k;return <button key={k} onClick={()=>setDraft(d=>({...d,type:k}))} style={{display:'flex',alignItems:'center',gap:6,height:28,padding:'0 10px',borderRadius:99,cursor:'pointer',font:'500 12px/1 var(--font-sans)',border:'1px solid '+(on?'rgba(255,255,255,.3)':'rgba(255,255,255,.08)'),background:on?'rgba(255,255,255,.08)':'transparent',color:on?'var(--text-strong)':'var(--text-muted)'}}><span style={{width:6,height:6,borderRadius:99,background:TK.TYPES[k][2]}}></span>{TK.TYPES[k][0]}</button>})}
              <span style={{width:1,background:'rgba(255,255,255,.08)',margin:'0 4px'}}></span>
              <span style={{display:'flex',alignItems:'center',gap:6,height:28,padding:'0 10px',borderRadius:99,border:'1px solid rgba(255,255,255,.08)',font:'400 12px/1 var(--font-sans)',color:'var(--text-body)'}}><Icon name="folder" size={12} color="var(--text-subtle)"/>{draft.project}</span>
              <span style={{display:'flex',alignItems:'center',gap:6,height:28,padding:'0 10px',borderRadius:99,border:'1px solid rgba(255,255,255,.08)',font:'400 12px/1 var(--font-sans)',color:'var(--text-body)'}}><Icon name="signal" size={12} color="var(--text-subtle)"/>{PRE[draft.prio][0]}</span>
            </div>
            <pre style={{margin:0,whiteSpace:'pre-wrap',font:'400 13px/1.65 var(--font-sans)',color:'var(--text-muted)',maxHeight:180,overflow:'auto'}} className="log-scroll">{draft.desc}</pre>
          </div>
          <div style={{display:'flex',alignItems:'center',gap:8,padding:'10px 12px 10px 16px',borderTop:'1px solid rgba(255,255,255,.06)',background:'rgba(0,0,0,.2)'}}><span style={{...lbl,fontSize:11}}>{left>0?'Encore '+left+' message'+(left>1?'s':'')+' pour préciser':'Limite atteinte'}</span><span style={{flex:1}}></span><Button size="sm" variant="inverse" icon="check" onClick={()=>onCreate(draft,files,model,P)}>Valider et créer le ticket</Button></div>
        </div>}
      </div>}
    </div>
    <div style={{padding:'0 24px 20px'}}><div style={{maxWidth:700,margin:'0 auto'}}>
      <div style={{display:'flex',flexDirection:'column',borderRadius:16,border:'1px solid rgba(255,255,255,.12)',background:'#121215',opacity:left<=0?.55:1}}>
        {pending.length>0&&<div style={{display:'flex',gap:8,flexWrap:'wrap',padding:'12px 12px 0'}}>{pending.map((f,i)=><span key={i} style={{position:'relative',display:'flex',alignItems:'center',gap:8,height:44,padding:f.url?0:'0 12px',borderRadius:10,overflow:'hidden',background:'rgba(255,255,255,.05)',border:'1px solid rgba(255,255,255,.08)'}}>{f.url?<img src={f.url} alt="" style={{width:60,height:44,objectFit:'cover'}}/>:<><Icon name="file-text" size={14} color="var(--text-muted)"/><span style={{font:'400 12px/1 var(--font-sans)',color:'var(--text-body)',maxWidth:140,overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap'}}>{f.name}</span></>}<button onClick={()=>setPending(p=>p.filter(x=>x!==f))} style={{position:f.url?'absolute':'static',top:3,right:3,width:18,height:18,borderRadius:99,border:0,cursor:'pointer',display:'flex',alignItems:'center',justifyContent:'center',background:'rgba(0,0,0,.6)'}}><Icon name="x" size={10} color="#fff"/></button></span>)}</div>}
        <textarea disabled={left<=0} value={v} onChange={e=>setV(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();send()}}} placeholder={left<=0?'Limite de 5 messages atteinte, validez le brouillon':msgs.length?'Ajoutez une précision…':'Ex. Le graphique de latence reste figé sur ciphera-web après 5 minutes…'} rows={msgs.length?1:2} style={{resize:'none',padding:'14px 16px 6px',background:'transparent',border:0,outline:'none',color:'var(--text-strong)',font:'400 14px/1.5 var(--font-sans)'}}></textarea>
        <div style={{display:'flex',alignItems:'center',gap:6,padding:'6px 8px 8px 8px'}}>
          <IconButton size={30} variant="ghost" icon="paperclip" label="Joindre des fichiers" onClick={()=>fileRef.current.click()}/>
          <input ref={fileRef} type="file" multiple style={{display:'none'}} onChange={e=>{addFiles(e.target.files);e.target.value=''}}/>
          {P&&<ModelPicker llms={llms} prov={P.id} setProv={setProv} model={model} setModel={setModel}/>}
          <span style={{flex:1}}></span>
          {msgs.length>0&&<span style={{...lbl,fontSize:11,marginRight:4}}>{left}/{MAX}</span>}
          <button onClick={send} disabled={busy||left<=0||(!v.trim()&&!pending.length)} style={{width:32,height:32,borderRadius:99,border:0,cursor:'pointer',display:'flex',alignItems:'center',justifyContent:'center',background:(v.trim()||pending.length)&&left>0&&!busy?'#ededed':'rgba(255,255,255,.08)'}}><Icon name="arrow-up" size={15} color={(v.trim()||pending.length)&&left>0&&!busy?'#0b0b0b':'var(--text-subtle)'}/></button>
        </div>
      </div>
      <span style={{display:'block',textAlign:'center',marginTop:10,...lbl,fontSize:11}}>Glissez des fichiers ici · L'IA peut se tromper, relisez le brouillon avant de valider</span>
    </div></div>
  </div>;
}
window.TicketAICompose=TicketAICompose;
})();
