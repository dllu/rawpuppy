#!/usr/bin/env python3
"""Compare owned native perspective previews against independent cooked exports.

Usage: python tools/score_perspective.py CORPUS RESULTS
Requires OpenCV and NumPy. Each corpus case contains case.json and reference.jpg.
The detector never receives the references. SIFT/MAGSAC finds a validation-only
mapping; canonical verticals measure alignment to that export's vertical axis,
which can differ from perfectly plumb architecture or an artist's partial edit.
"""
from pathlib import Path
import json, sys
import cv2
import numpy as np
cv2.setNumThreads(8)
r=Path(sys.argv[1]);results=Path(sys.argv[2]);oracle=r/'oracle';oracle.mkdir(exist_ok=True)
sift=cv2.SIFT_create(nfeatures=10000,contrastThreshold=.02)

def transform(h,p):
 q=np.c_[np.asarray(p).reshape(-1,2),np.ones(len(p))]@h.T
 return q[:,:2]/q[:,2:]

def geometry(g,aspect):
 sy,sp,sr=np.sin(np.deg2rad([g['yaw'],g['pitch'],g['rotation']]))
 cy,cp,cr=np.cos(np.deg2rad([g['yaw'],g['pitch'],g['rotation']]))
 f=.5/np.tan(np.deg2rad(g['field_of_view'])/2)
 k=np.array([[f,0,.5],[0,f/aspect,.5],[0,0,1]])
 sx,sy_=g.get('shift',[0,0]);sc=g['scale']
 ki=np.array([[1/(f*sc),0,(-.5+sx)/(f*sc)],[0,aspect/(f*sc),(-.5+sy_)*aspect/(f*sc)],[0,0,1]])
 rot=np.array([[cy,0,sy],[0,1,0],[-sy,0,cy]])@np.array([[1,0,0],[0,cp,-sp],[0,sp,cp]])@np.array([[cr,-sr,0],[sr,cr,0],[0,0,1]])
 return k@rot@ki

def reference(case,folder):
 op=oracle/(case.name+'.json')
 if op.exists():
  old=json.loads(op.read_text())
  if 'canonical_source_verticals' in old:return old
 src=cv2.imread(str(folder/'analysis.png'),cv2.IMREAD_GRAYSCALE);dst=cv2.imread(str(case/'reference.jpg'),cv2.IMREAD_GRAYSCALE)
 ka,da=sift.detectAndCompute(src,None);kb,db=sift.detectAndCompute(dst,None)
 ms=cv2.BFMatcher().knnMatch(da,db,k=2);good=[a for a,b in ms if a.distance<.72*b.distance]
 a=np.float32([ka[m.queryIdx].pt for m in good]);b=np.float32([kb[m.trainIdx].pt for m in good])
 h,ok=cv2.findHomography(a,b,cv2.USAC_MAGSAC,2.,maxIters=10000,confidence=.9999)
 sel=ok[:,0]>0;err=np.linalg.norm(transform(h,a[sel])-b[sel],axis=1)
 lines=cv2.createLineSegmentDetector(0).detect(dst)[0];lines=np.empty((0,4)) if lines is None else lines.reshape(-1,4)
 selected=[]
 for l in lines:
  x1,y1,x2,y2=l;dx=x2-x1;dy=y2-y1;length=np.hypot(dx,dy)
  if length<max(35,.025*np.hypot(*dst.shape)):continue
  if abs(np.degrees(np.arctan2(abs(dx),abs(dy))))>.4:continue
  selected.append(l)
 # Long independent edge locations, avoiding two sides of the same narrow frame.
 selected.sort(key=lambda l:-np.hypot(l[2]-l[0],l[3]-l[1]));unique=[]
 for l in selected:
  if any(abs((l[0]+l[2])/2-(p[0]+p[2])/2)<5 and abs((l[1]+l[3])/2-(p[1]+p[3])/2)<(abs(l[3]-l[1])+abs(p[3]-p[1]))*.6 for p in unique):continue
  unique.append(l)
 endpoints=np.array(unique).reshape(-1,2)
 ideal=np.array(unique).reshape(-1,2,2).copy();ideal[:,:,0]=ideal[:,:,0].mean(axis=1)[:,None]
 source=transform(np.linalg.inv(h),endpoints).reshape(-1,4)
 canonical=transform(np.linalg.inv(h),ideal.reshape(-1,2)).reshape(-1,4)
 data={'matches':len(good),'inliers':int(sel.sum()),'median_match_error':float(np.median(err)),'source_size':[src.shape[1],src.shape[0]],'reference_size':[dst.shape[1],dst.shape[0]],'homography':h.tolist(),'reference_verticals':np.array(unique).tolist(),'source_verticals':source.tolist(),'canonical_source_verticals':canonical.tolist(),'inlier_span':np.ptp(a[sel],axis=0).tolist()}
 op.write_text(json.dumps(data,indent=2));return data

scores=[]
for folder in sorted(results.iterdir()):
 case=r/folder.name
 if not (folder/'analysis.png').exists() or not (case/'reference.jpg').exists():continue
 try:d=reference(case,folder)
 except Exception as e:print('reference failed',case.name,e,flush=True);continue
 w,h=d['source_size'];points=np.array(d.get('canonical_source_verticals',d['source_verticals'])).reshape(-1,2)/[w,h]
 result={'id':case.name,'split':json.load(open(case/'case.json'))['split'],'reference':{k:d[k] for k in ['matches','inliers','median_match_error']},'vertical_edges':len(points)//2,'outcomes':{}}
 for file in folder.glob('*-vertical.json'):
  report=json.load(open(file));g=report['solution']
  if g:
   q=transform(np.linalg.inv(geometry(g,h/w)),points).reshape(-1,2,2);v=(q[:,1]-q[:,0])*[w,h]
   angles=np.degrees(np.arctan2(abs(v[:,0]),abs(v[:,1])))
   result['outcomes'][file.stem]={'median_lean':float(np.median(angles)),'p90_lean':float(np.quantile(angles,.9)),'maximum_lean':float(max(angles)),'angles':angles.tolist()}
  else:result['outcomes'][file.stem]={'abstained':report['reason']}
 print(case.name, 'matches',d['inliers'], 'median reprojection',round(d['median_match_error'],3), 'verticals',len(points)//2, {k: {a:round(b,3) if isinstance(b,float) else b for a,b in v.items() if a!='angles'} for k,v in result['outcomes'].items()},flush=True)
 scores.append(result)
(results/'scores.json').write_text(json.dumps(scores,indent=2))
