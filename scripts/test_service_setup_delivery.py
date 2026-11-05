"""Verify the service-setup workflow against a supplied (including installed) zi.

Only Python's standard library, synthetic configuration and loopback fixtures are
used. Python is a test-host dependency, never a zi runtime dependency. No real
provider credentials, services, project files or user settings are read.
"""
import argparse
from contextlib import contextmanager
import http.server
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import tomllib


INPUT = b'PRIVATE_INPUT_993 alpha\n'
KEY = 'DELIVERY_KEY_993'
BUDGET_FIELDS = {'max_tokens', 'max_completion_tokens', 'max_output_tokens'}


def answer(api, text, tool=False):
    if api == 'responses':
        content = ([{'type': 'function_call', 'call_id': 'delivery-call',
                     'name': 'count_input', 'arguments': '{}'}] if tool else
                   [{'type': 'message', 'content': [{'type': 'output_text', 'text': text}]}])
        return {'status': 'completed', 'output': content}
    if api == 'anthropic':
        content = ([{'type': 'tool_use', 'id': 'delivery-call', 'name': 'count_input', 'input': {}}]
                   if tool else [{'type': 'text', 'text': text}])
        return {'stop_reason': 'tool_use' if tool else 'end_turn', 'content': content}
    message = ({'role': 'assistant', 'content': None, 'tool_calls': [
        {'id': 'delivery-call', 'type': 'function',
         'function': {'name': 'count_input', 'arguments': '{}'}}]} if tool else
        {'role': 'assistant', 'content': text})
    return {'choices': [{'message': message, 'finish_reason': 'tool_calls' if tool else 'stop'}]}


def stream_answer(api, text):
    def event(value):
        return 'data: ' + json.dumps(value, ensure_ascii=False) + '\n\n'
    if api == 'responses':
        return event({'type': 'response.output_text.delta', 'delta': text}) + event({
            'type': 'response.completed', 'response': answer(api, text)})
    if api == 'anthropic':
        return ''.join(map(event, [
            {'type': 'message_start', 'message': {'role': 'assistant', 'content': [], 'usage': {}}},
            {'type': 'content_block_start', 'index': 0, 'content_block': {'type': 'text', 'text': ''}},
            {'type': 'content_block_delta', 'index': 0, 'delta': {'type': 'text_delta', 'text': text}},
            {'type': 'content_block_stop', 'index': 0},
            {'type': 'message_delta', 'delta': {'stop_reason': 'end_turn'}},
            {'type': 'message_stop'},
        ]))
    return event({'choices': [{'index': 0, 'delta': {'content': text}, 'finish_reason': None}]}) + event({
        'choices': [{'index': 0, 'delta': {}, 'finish_reason': 'stop'}]}) + 'data: [DONE]\n\n'


@contextmanager
def service(api, model):
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_GET(self):
            self.handle_request('GET')

        def do_POST(self):
            self.handle_request('POST')

        def handle_request(self, method):
            try:
                assert len(server.requests) < len(server.plan), 'unexpected extra service request'
                expected, inspect = server.plan[len(server.requests)]
                assert expected == method, 'unexpected service method'
                if api == 'anthropic':
                    assert self.headers.get('x-api-key') == KEY
                    assert self.headers.get('anthropic-version') == '2023-06-01'
                    assert self.headers.get('authorization') is None
                else:
                    assert self.headers.get('authorization') == 'Bearer ' + KEY
                    assert self.headers.get('x-api-key') is None
                if method == 'GET':
                    assert self.path == '/v1/models' + ('?limit=1000' if api == 'anthropic' else '')
                    assert self.headers.get('content-length') is None
                    body = None
                else:
                    assert self.path == '/v1/' + {'chat': 'chat/completions',
                           'responses': 'responses', 'anthropic': 'messages'}[api]
                    size = int(self.headers['Content-Length'])
                    assert 0 < size < 131072
                    body = json.loads(self.rfile.read(size))
                    assert body['model'] == model
                status, content_type, response = inspect(body)
                server.requests.append((method, body))
                encoded = response.encode('utf-8')
                self.send_response(status)
                self.send_header('Content-Type', content_type)
                self.send_header('Content-Length', str(len(encoded)))
                self.end_headers()
                self.wfile.write(encoded)
            except Exception as error:
                server.errors.append(type(error).__name__)
                # Avoid printing callback inputs, credential headers or bodies.
                self.close_connection = True

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    server.daemon_threads = True
    server.requests, server.plan, server.errors = [], [], []
    worker = threading.Thread(target=server.serve_forever, kwargs={'poll_interval': .01}, daemon=True)
    worker.start()
    try:
        yield server, f'http://127.0.0.1:{server.server_port}/v1'
        assert not server.errors, 'controlled service assertion failed: ' + ','.join(server.errors)
        assert len(server.requests) == len(server.plan), 'missing service requests'
    finally:
        server.shutdown()
        server.server_close()
        worker.join()


def check_budget(body, field, tokens):
    assert {key: body[key] for key in BUDGET_FIELDS if key in body} == {field: tokens}


def json_reply(value, status=200):
    return status, 'application/json', json.dumps(value, ensure_ascii=False)


def verify(binary):
    checks, total_requests = [], 0
    variants = [('chat', None, 'max_completion_tokens'), ('chat', 'max_tokens', 'max_tokens'),
                ('responses', None, 'max_output_tokens'), ('anthropic', None, 'max_tokens')]
    for api, preference, wire in variants:
        label = api + ('/legacy' if preference else '')
        model = 'delivery/' + label
        with tempfile.TemporaryDirectory(prefix='zi service-测试 ') as temporary, service(api, model) as (server, url):
            root = Path(temporary)
            config = root / 'model 配置.toml'
            env = {key: value for key, value in os.environ.items()
                   if not key.upper().startswith('ZI_') and key.upper() not in {
                       'OPENAI_API_KEY', 'ANTHROPIC_API_KEY', 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY'}}
            env.update(HOME=str(root), USERPROFILE=str(root), APPDATA=str(root / 'config'),
                       XDG_CONFIG_HOME=str(root / 'config'), NO_PROXY='*', DELIVERY_MODEL_KEY=KEY)
            config.write_text(f'''model='root-model'
base_url='http://127.0.0.1:1/v1'
api_key='ROOT_KEY_993'
[profiles.work]
model='{model}'
base_url='{url}'
api='{api}'
api_key='SAVED_KEY_993'
api_key_env='DELIVERY_MODEL_KEY'
''', encoding='utf-8')

            def read():
                return tomllib.loads(config.read_text(encoding='utf-8'))

            def run(args, data=b'', success=True, allow_input=False):
                unchanged = config.read_bytes() if args[0] not in {'use', 'config'} else None
                result = subprocess.run([str(binary), '--config', str(config), '--timeout', '3', *map(str, args)],
                                        input=data, capture_output=True, cwd=root, env=env, timeout=15)
                assert (result.returncode == 0) == success, (label, 'unexpected command exit', result.returncode)
                assert not server.errors, (label, 'controlled request failed', server.errors)
                if unchanged is not None:
                    assert config.read_bytes() == unchanged, 'read-only command modified configuration'
                for secret in [KEY, 'ROOT_KEY_993', 'SAVED_KEY_993', 'PROVIDER_BODY_993']:
                    assert secret.encode() not in result.stdout + result.stderr, 'credential/body disclosure'
                if not allow_input:
                    assert b'PRIVATE_INPUT_993' not in result.stdout + result.stderr, 'unexpected input disclosure'
                return result

            baseline = read()
            run(['use', 'work'])
            args = ['config', '--max-output-tokens', '128']
            if preference:
                args += ['--chat-token-limit-field', preference]
            run(args)
            expected = baseline.copy()
            expected['profiles'] = {'work': dict(baseline['profiles']['work'], max_output_tokens=128)}
            expected['active_profile'] = 'work'
            if preference:
                expected['profiles']['work']['chat_token_limit_field'] = preference
            assert read() == expected and not server.requests
            report = json.loads(run(['doctor', '--json'], INPUT).stdout)
            details = report['configuration']['details']
            assert report['ok'] and report['probe']['status'] == 'not_requested'
            assert details['profile'] == 'work' and details['model'] == model
            assert details['credential'] == {'configured': True, 'source': 'environment',
                                             'environment_variable': 'DELIVERY_MODEL_KEY'}
            assert details['output_budget']['tokens_per_turn'] == 128 and details['output_budget']['field'] == wire
            assert details['output_budget']['tokens_source'] == 'profile' and not server.requests
            checks.append(label + ': select/save budget/offline doctor use the same isolated profile and key source')

            def catalogue(_):
                value = {'data': [{'id': model}]}
                if api == 'anthropic':
                    value.update(has_more=False, first_id=model, last_id=model)
                return json_reply(value)
            server.plan.append(('GET', catalogue))
            value = json.loads(run(['models', '--remote', '--json'], INPUT).stdout)
            assert value['profile'] == 'work' and value['protocol'] == api and value['models'] == [model]
            assert value['complete'] and read() == expected
            checks.append(label + ': catalogue discovers the configured ID without input disclosure or saving')

            def probe(body):
                check_budget(body, wire, 16)
                assert not body.get('stream') and not body.get('tools')
                assert 'PRIVATE_INPUT_993' not in json.dumps(body)
                return json_reply(answer(api, 'OK'))
            server.plan.append(('POST', probe))
            report = json.loads(run(['doctor', '--json', '--probe'], INPUT).stdout)
            assert report['ok'] and report['probe']['status'] == 'ok'
            assert report['probe']['max_output_tokens'] == 16 and report['probe']['field'] == wire
            checks.append(label + ': explicit probe is minimal fixed16 while saved generation cap remains128')

            task = 'Count the input words with the native tool; return words=2.'
            before = len(server.requests)
            preview = json.loads(run(['--preview', '--no-stream', task], INPUT, allow_input=True).stdout)
            assert preview['network_requests'] == 0 and len(server.requests) == before
            first = preview['request']['body']
            check_budget(first, wire, 128)
            assert 'PRIVATE_INPUT_993' in json.dumps(first)

            def initial(body):
                assert body == first, 'preview and actual first request differ'
                return json_reply(answer(api, '', tool=True))

            def followup(body):
                check_budget(body, wire, 128)
                if api == 'responses':
                    output = body['input'][-1]
                    assert output['type'] == 'function_call_output' and output['call_id'] == 'delivery-call'
                    raw = output['output']
                elif api == 'anthropic':
                    output = body['messages'][-1]['content'][0]
                    assert output['type'] == 'tool_result' and output['tool_use_id'] == 'delivery-call'
                    raw = output['content']
                else:
                    output = body['messages'][-1]
                    assert output['role'] == 'tool' and output['tool_call_id'] == 'delivery-call'
                    raw = output['content']
                result = json.loads(raw)
                assert result['words'] == 2 and result['bytes'] == len(INPUT)
                assert result['characters'] == len(INPUT.decode()) and result['lines'] == 1
                return json_reply(answer(api, 'words=2'))
            server.plan.extend([('POST', initial), ('POST', followup)])
            destination = root / 'result 输出.txt'
            result = run(['--no-stream', '-o', destination, task], INPUT)
            assert not result.stdout and destination.read_bytes() == b'words=2\n'
            assert len(server.requests) == before + 2 and read() == expected
            checks.append(label + ': preview equals actual request, native count and followup retain cap and file output')

            def streaming(body):
                check_budget(body, wire, 256)
                assert body['stream'] and not body.get('tools')
                return 200, 'text/event-stream', stream_answer(api, 'streamed-甲')
            server.plan.append(('POST', streaming))
            result = run(['--stream', '--no-tools', '--max-output-tokens', '256', task], INPUT)
            assert result.stdout == 'streamed-甲\n'.encode() and not result.stderr and read() == expected
            checks.append(label + ': explicit stream and CLI cap preserve exact Unicode pipe output without persisting override')

            def denied(body):
                check_budget(body, wire, 128)
                return json_reply({'error': {'message': KEY + ' PROVIDER_BODY_993 PRIVATE_INPUT_993'}}, 403)
            before = len(server.requests)
            server.plan.append(('POST', denied))
            result = run(['--no-stream', '-o', destination, '--force', task], INPUT, success=False)
            assert not result.stdout and b'403' in result.stderr and destination.read_bytes() == b'words=2\n'
            assert len(server.requests) == before + 1 and read() == expected
            checks.append(label + ': denied task emits no body/prefix, does not retry, preserves existing result and saved cap')

            run(['config', '--clear-output-budget'])
            expected['profiles']['work'].pop('max_output_tokens')
            expected['profiles']['work'].pop('chat_token_limit_field', None)
            assert read() == expected
            report = json.loads(run(['doctor', '--json']).stdout)
            budget = report['configuration']['details']['output_budget']
            assert budget['tokens_per_turn'] == (4096 if api == 'anthropic' else None)
            assert budget['tokens_source'] == 'default'
            run(['use', 'default'])
            expected.pop('active_profile')
            report = json.loads(run(['doctor', '--json']).stdout)
            assert report['configuration']['details']['profile'] == 'default' and read() == expected
            assert len(server.requests) == len(server.plan) == 6
            assert not (root / 'config').exists(), 'unexpected default configuration/history directory'
            checks.append(label + ': clear preferences and return to default offline; credentials, other settings and file retained')
            total_requests += len(server.requests)
    assert len(checks) == 28 and total_requests == 24, 'incomplete service delivery verification'
    return {'passed': len(checks), 'checks': checks, 'synthetic_loopback_requests': total_requests,
            'real_provider_requests': 0}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    arguments = parser.parse_args()
    print(json.dumps(verify(arguments.binary.resolve(strict=True)), ensure_ascii=True))
