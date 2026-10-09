"""Sample a Darktable preset as an oracle; only generated numeric output is packed.

The application never loads Darktable or its source. This tool invokes existing
executables with independent RGB probes and an isolated configuration/database.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import xml.etree.ElementTree as ET


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--darktable', required=True, type=Path)
    parser.add_argument('--preset', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--grid', type=int, default=97)
    args = parser.parse_args()
    namespace = {'d': 'http://darktable.sf.net/', 'r': 'http://www.w3.org/1999/02/22-rdf-syntax-ns#'}
    d = '{' + namespace['d'] + '}'
    r = '{' + namespace['r'] + '}'
    ET.register_namespace('x','adobe:ns:meta/')
    ET.register_namespace('rdf',namespace['r'])
    ET.register_namespace('darktable',namespace['d'])
    source = ET.parse(args.preset)
    module = next(node for node in source.getroot().iter() if node.get(d+'operation') == 'agx')
    module.set(d+'num', '0')
    root = ET.Element('{adobe:ns:meta/}xmpmeta')
    desc = ET.SubElement(ET.SubElement(root,r+'RDF'),r+'Description',{
        r+'about':'',d+'xmp_version':'5',d+'auto_presets_applied':'1',
        d+'history_end':'1',d+'iop_order_version':'1'})
    ET.SubElement(ET.SubElement(desc,d+'history'),r+'Seq').append(module)
    env = dict(os.environ, OMP_NUM_THREADS='8')
    with tempfile.TemporaryDirectory(prefix='rawpuppy-agx-') as temporary:
        work = Path(temporary)
        ET.ElementTree(root).write(work/'preset.xmp',encoding='utf-8',xml_declaration=True)
        for name in ['config','cache']:
            (work/name).mkdir()
        subprocess.run(['cargo','run','--example','agx_lattice','--','prepare',
                        str(work/'input.exr'),'--grid',str(args.grid),'--tagged'],check=True)
        command = [str(args.darktable),str(work/'input.exr'),str(work/'preset.xmp'),
                   str(work/'output.exr'),'--apply-custom-presets','false','--icc-type','LIN_REC2020',
                   '--core','--configdir',str(work/'config'),'--cachedir',str(work/'cache'),
                   '--library',':memory:','--disable-opencl','--conf','write_sidecar_files=never',
                   '--conf','plugins/darkroom/workflow=none','--conf','plugins/imageio/format/exr/bpp=32']
        subprocess.run(command,env=env,check=True)
        args.output.parent.mkdir(parents=True,exist_ok=True)
        subprocess.run(['cargo','run','--example','agx_lattice','--','pack',
                        str(work/'output.exr'),str(args.output),'--grid',str(args.grid)],check=True)
    manifest = {
        'version':1,'grid':args.grid,'input_basis':'linear Rec.2020, with explicit EXR chromaticities',
        'output_basis':'linear Rec.2020','order':'red fastest, then green, then blue; RGB float32 little endian',
        'shaper':'0..0.5: log2(1 + x/(0.18/1024))/log2(1025)/2; 0.5..1: 0.5 + log2(x/0.18)/32',
        'oracle_version':subprocess.check_output([str(args.darktable),'--version'],text=True).splitlines()[0],
        'oracle_executable_sha256':digest(args.darktable),'preset_sha256':digest(args.preset),
        'preset_name':module.get(d+'multi_name'),'preset_params':module.get(d+'params'),
        'sha256':digest(args.output)}
    args.output.with_suffix('.json').write_text(json.dumps(manifest,indent=2)+'\n')


if __name__ == '__main__':
    main()
