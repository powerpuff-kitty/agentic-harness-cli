#!/usr/bin/env python3
import json
from pathlib import Path
from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = json.loads((ROOT / 'catalog/schema/source-graph.v1.schema.json').read_text())
Draft202012Validator.check_schema(SCHEMA)
VALIDATOR = Draft202012Validator(SCHEMA)
D = 'sha256:' + 'a' * 64


def frontend(language, implementation, extensions):
    return {
        'language': language,
        'implementation': implementation,
        'version': 'fixture-1',
        'extensions': extensions,
        'capabilities': [
            {'capability': 'parse', 'support': 'supported', 'note': None},
            {'capability': 'imports', 'support': 'supported', 'note': None},
            {'capability': 'packages', 'support': 'partial', 'note': 'fixture capability'},
        ],
    }


def graph(language='typescript', implementation='oxc', extensions=None):
    extensions = extensions or ['ts', 'tsx']
    return {
        'format_version': 1,
        'kind': 'source-graph',
        'source_digest': D,
        'frontends': [frontend(language, implementation, extensions)],
        'nodes': [
            {'id': 'file:src/main', 'path': 'src/main.ts', 'language': language, 'kind': 'file', 'generated': False, 'metadata': {}},
            {'id': 'file:src/service', 'path': 'src/service.ts', 'language': language, 'kind': 'file', 'generated': False, 'metadata': {}},
        ],
        'edges': [
            {'from': 'file:src/main', 'to': 'file:src/service', 'kind': 'runtime', 'resolution': 'local', 'specifier': './service', 'line': 1}
        ],
        'coverage': {'files_discovered': 2, 'files_parsed': 2, 'files_failed': 0, 'edges_resolved': 1, 'edges_unresolved': 0, 'complete': True},
        'not_checked': ['compiler-grade type semantics'],
    }


def semantic_errors(value):
    errors = []
    node_ids = [node['id'] for node in value['nodes']]
    node_set = set(node_ids)
    if len(node_ids) != len(node_set):
        errors.append('duplicate node id')

    frontend_languages = [entry['language'] for entry in value['frontends']]
    if len(frontend_languages) != len(set(frontend_languages)):
        errors.append('duplicate frontend language')
    supported_languages = set(frontend_languages)

    for node in value['nodes']:
        if node['language'] not in supported_languages:
            errors.append(f"node language has no frontend: {node['id']}")

    resolved = 0
    unresolved = 0
    for edge in value['edges']:
        if edge['from'] not in node_set:
            errors.append(f"edge source is unknown: {edge['from']}")
        if edge['resolution'] == 'unresolved':
            unresolved += 1
            if edge['to'] is not None:
                errors.append('unresolved edge must not claim a target')
        else:
            resolved += 1
            if edge['resolution'] in {'local', 'workspace'} and edge['to'] not in node_set:
                errors.append('local/workspace edge target is unknown')
            if edge['resolution'] in {'external', 'resource'} and edge['to'] is not None:
                errors.append('external/resource edge must not claim a graph-local target')

    coverage = value['coverage']
    if coverage['files_parsed'] + coverage['files_failed'] != coverage['files_discovered']:
        errors.append('file coverage arithmetic mismatch')
    if coverage['edges_resolved'] != resolved or coverage['edges_unresolved'] != unresolved:
        errors.append('edge coverage arithmetic mismatch')
    if coverage['complete'] and (coverage['files_failed'] != 0 or unresolved != 0):
        errors.append('complete graph contains failed files or unresolved edges')
    return errors


def validate(value):
    VALIDATOR.validate(value)
    errors = semantic_errors(value)
    if errors:
        raise AssertionError('; '.join(errors))


for language, implementation, extensions in [
    ('typescript', 'oxc', ['ts', 'tsx']),
    ('python', 'fixture-python', ['py']),
    ('rust', 'fixture-rust', ['rs']),
    ('go', 'fixture-go', ['go']),
]:
    value = graph(language, implementation, extensions)
    value['nodes'][0]['path'] = f'src/main.{extensions[0]}'
    value['nodes'][1]['path'] = f'src/service.{extensions[0]}'
    validate(value)

for mutation in ['wrong-version', 'unknown-kind', 'invalid-resolution', 'negative-coverage']:
    value = graph()
    if mutation == 'wrong-version':
        value['format_version'] = 2
    elif mutation == 'unknown-kind':
        value['edges'][0]['kind'] = 'magic'
    elif mutation == 'invalid-resolution':
        value['edges'][0]['resolution'] = 'guessed'
    else:
        value['coverage']['files_failed'] = -1
    assert not VALIDATOR.is_valid(value), mutation

for mutation in [
    'duplicate-node', 'unknown-source', 'unknown-target', 'unresolved-target',
    'coverage-files', 'coverage-edges', 'false-complete', 'missing-frontend',
]:
    value = graph()
    if mutation == 'duplicate-node':
        value['nodes'].append(dict(value['nodes'][0]))
    elif mutation == 'unknown-source':
        value['edges'][0]['from'] = 'file:missing'
    elif mutation == 'unknown-target':
        value['edges'][0]['to'] = 'file:missing'
    elif mutation == 'unresolved-target':
        value['edges'][0]['resolution'] = 'unresolved'
        value['coverage']['edges_resolved'] = 0
        value['coverage']['edges_unresolved'] = 1
        value['coverage']['complete'] = False
    elif mutation == 'coverage-files':
        value['coverage']['files_parsed'] = 1
    elif mutation == 'coverage-edges':
        value['coverage']['edges_resolved'] = 0
    elif mutation == 'false-complete':
        value['edges'][0]['resolution'] = 'unresolved'
        value['edges'][0]['to'] = None
        value['coverage']['edges_resolved'] = 0
        value['coverage']['edges_unresolved'] = 1
    else:
        value['nodes'][0]['language'] = 'python'
    assert semantic_errors(value), mutation

partial = graph()
partial['edges'][0]['resolution'] = 'unresolved'
partial['edges'][0]['to'] = None
partial['coverage']['edges_resolved'] = 0
partial['coverage']['edges_unresolved'] = 1
partial['coverage']['complete'] = False
validate(partial)

print('Source Graph v1 schema, semantic integrity and cross-language fixtures passed')
