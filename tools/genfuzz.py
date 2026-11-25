# Random ProTracker M.K. modules that exercise every effect (complete, untruncated files)
import random, struct, sys
PER=[856,808,762,720,678,640,604,570,538,508,480,453,428,404,381,360,339,320,302,285,269,254,240,226,214,202,190,180,170,160,151,143,135,127,120,113]
def gen(seed, path):
    R=random.Random(seed)
    nsmp=R.randint(1,31); samples=[]; hdr=bytearray(b'fuzz%-16d'%seed)
    for i in range(31):
        if i<nsmp:
            L=R.choice([0,2,4,R.randint(1,200),R.randint(200,4000),R.randint(4000,32767)])
            ls=R.randint(0,max(0,L-1)) if R.random()<.5 else 0
            ll=R.choice([1,R.randint(1,max(1,L-ls)),R.randint(1,L+50 if L else 2)])
            kind=R.randint(0,3)
            if kind==0: data=bytes(R.getrandbits(8) for _ in range(L*2))
            elif kind==1: data=bytes(((j*R.randint(1,9))&0xff) for j in range(L*2))
            elif kind==2: data=bytes((127 if (j//R.randint(2,40))%2 else 129) for j in range(L*2))
            else: data=bytes((int(100*__import__('math').sin(j/7.0))&0xff) for j in range(L*2))
            samples.append(data)
            hdr+=b'smp%-19d'%i+struct.pack('>HBBHH',L,R.randint(0,15),R.choice([R.randint(0,64),R.randint(0,255)]),ls,ll)
        else:
            samples.append(b''); hdr+=bytes(22)+struct.pack('>HBBHH',0,0,0,0,1)
    npat=R.randint(1,6); slen=R.randint(1,12)
    order=[R.randint(0,npat-1) for _ in range(slen)]
    hdr+=bytes([slen,127])+bytes(order+[0]*(128-slen))+b'M.K.'
    npat=max(order)+1
    pats=bytearray()
    for p in range(npat):
        for r in range(64):
            for c in range(4):
                per=0; s=0
                if R.random()<.45:
                    per=R.choice(PER) if R.random()<.95 else R.randint(1,1023)
                if R.random()<.5: s=R.randint(0,31)
                cmd=0; prm=0
                if R.random()<.7:
                    cmd=R.randint(0,15)
                    prm=R.getrandbits(8)
                    if cmd==0xF: prm=R.choice([R.randint(1,31),R.randint(32,255),R.randint(1,10),0 if R.random()<.05 else 6])
                    if cmd==0xB: prm=R.randint(0,slen+2) if R.random()<.3 else 0; cmd=cmd if R.random()<.15 else 0xA
                    if cmd==0xD and R.random()<.7: cmd=0xA
                    if cmd==0xE:
                        x=R.randint(0,15)
                        if x==6 and R.random()<.3: pass
                        elif x==0xE: prm=(x<<4)|R.randint(0,4)
                        prm=(x<<4)|(prm&15)
                pats+=bytes([(s&0xf0)|(per>>8), per&0xff, ((s&15)<<4)|cmd, prm])
    open(path,'wb').write(bytes(hdr)+bytes(pats)+b''.join(samples))
n=int(sys.argv[1]); base=int(sys.argv[2]) if len(sys.argv)>2 else 0
for i in range(n): gen(base+i, '/home/claude/fuzz/f%05d.mod'%(base+i))
