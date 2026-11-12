"""Check an installed executable's complete table-preparation workflow offline.

Uses only the Python standard library and synthetic inputs. This is a delivery
check, not a zi runtime dependency or a model-service quality evaluation.
"""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile


def verify(binary):
    checks = []
    with tempfile.TemporaryDirectory(prefix='zi table-测试 ') as temporary:
        root = Path(temporary)
        env = {k: v for k, v in os.environ.items()
               if not k.upper().startswith('ZI_') and k.upper() not in {
                   'OPENAI_API_KEY', 'ANTHROPIC_API_KEY',
                   'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY'}}
        env.update(HOME=str(root), USERPROFILE=str(root), APPDATA=str(root/'config'),
                   XDG_CONFIG_HOME=str(root/'config'), NO_PROXY='*')
        broken = root/'unused model.toml'
        broken.write_text('invalid model configuration', encoding='utf-8')
        env['ZI_CONFIG'] = str(broken)
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            listener.listen()
            listener.setblocking(False)
            env['ZI_BASE_URL'] = f'http://127.0.0.1:{listener.getsockname()[1]}/v1'
            env['ZI_MODEL'] = 'delivery-fixture'

            def run(args, data=b'', success=True):
                result = subprocess.run([str(binary), *map(str, args)], input=data,
                                        capture_output=True, cwd=root, env=env, timeout=30)
                assert (result.returncode == 0) == success, (args[:3], result.returncode)
                try:
                    connection, _ = listener.accept()
                except BlockingIOError:
                    pass
                else:
                    connection.close()
                    raise AssertionError('Offline table workflow attempted a model connection')
                return result

            recipe = root/'table recipe-测试.toml'
            recipe.write_text("""version=1
[[steps]]
operation="tsv-to-jsonl"
[[steps]]
operation="jsonl-count-by"
pointer="/level"
[[steps]]
operation="jsonl-where-int"
pointer="/count"
equals="2"
[[steps]]
operation="jsonl-project"
fields=[{name="level",pointer="/value"},{name="count",pointer="/count"},{name="note",pointer="/note",default_json='""'}]
[[steps]]
operation="jsonl-to-tsv"
""", encoding='utf-8')
            raw = b'id\tlevel\r\na\tERROR\r\nb\tINFO\r\nc\tERROR\r\n'
            expected = b'"level"\t"count"\t"note"\n"ERROR"\t"2"\t""\n'
            result = run(['data', '--recipe', recipe], raw)
            assert result.stdout == expected and not result.stderr
            checks.append('compound recipe: TSV/count/exact integer equality/explicit default/TSV')
            source = root/'input 表格.tsv'
            source.write_bytes(raw)
            destination = root/'output 表格.tsv'
            result = run(['data', '--recipe', recipe, '-i', source, '-o', destination])
            assert not result.stdout and not result.stderr and destination.read_bytes() == expected
            checks.append('file input/output: exact pipe bytes with Unicode paths')
            exported = run(['data', '--show-plan', '--recipe', recipe], b'not table data')
            assert not exported.stderr
            replay = root/'reviewed.toml'
            replay.write_bytes(exported.stdout)
            assert run(['data', '--recipe', replay], raw).stdout == expected
            log_plan = run(['data', '--show-plan', '--recipe', 'builtin:top-error-codes'], b'not log data')
            assert not log_plan.stderr
            assert b'operation = "jsonl-where"' in log_plan.stdout
            log_recipe = root/'error report-测试.toml'
            original = log_plan.stdout
            modified = original.replace(b'equals = "ERROR"\n', b'equals = "ERROR"\nif_present = true\n', 1)
            assert modified != original
            log_recipe.write_bytes(modified)
            logs = b'{"level":"ERROR","error":{"code":"TIMEOUT"}}\n{}\n{"level":"ERROR"}\n{"level":"INFO"}\n'
            assert not run(['data', '--recipe', 'builtin:top-error-codes'], logs, success=False).stdout
            assert not run(['data', '--recipe', log_recipe], logs, success=False).stdout
            optional_count = modified.replace(b'pointer = "/error/code"\n', b'pointer = "/error/code"\nif_present = true\n', 1)
            assert optional_count != modified
            log_recipe.write_bytes(optional_count)
            result = run(['data', '--recipe', log_recipe], logs)
            assert result.stdout == b'{"value":"TIMEOUT","count":1}\n' and not result.stderr
            context = run(['data', 'lines-include', '--contains', 'ERROR', '--context', '1', '--show-plan'])
            assert not context.stderr and b'context = 1' in context.stdout
            context_recipe = root/'stack context-测试.toml'
            context_recipe.write_bytes(context.stdout)
            excerpt = run(['data', '--recipe', context_recipe], b'heading\r\nERROR\r\n at A\r\nother')
            assert excerpt.stdout == b'heading\nERROR\n at A\n' and not excerpt.stderr
            http_range = run(['data', 'jsonl-where-int', '--pointer', '/status',
                              '--at-least', '500', '--at-most', '599', '--show-plan'])
            range_recipe = root/'HTTP status-测试.toml'
            range_recipe.write_bytes(http_range.stdout)
            range_result = run(['data', '--recipe', range_recipe],
                               b'{"status":499}\n{"status":500}\n{"status":599}\n{"status":600}\n')
            assert range_result.stdout == b'{"status":500}\n{"status":599}\n' and not range_result.stderr
            http_logs = (b'{"status":499}\n{"status":500,"error":{"code":"TIMEOUT"}}\n'
                         b'{"status":503}\n{"message":"healthcheck"}\n'
                         b'{"status":599,"error":{"code":"TIMEOUT"}}\n'
                         b'{"status":502,"error":{"code":"BAD_GATEWAY"}}\n{"status":600}\n')
            report_plan = run(['data', '--show-plan', '--recipe', 'builtin:top-http-5xx-codes'], b'not JSONL')
            assert not report_plan.stderr
            report_recipe = root/'HTTP report-测试.toml'
            report_recipe.write_bytes(report_plan.stdout)
            expected_http = b'{"value":"TIMEOUT","count":2}\n{"value":"BAD_GATEWAY","count":1}\n'
            for definition in ['builtin:top-http-5xx-codes', report_recipe]:
                report = run(['data', '--recipe', definition], http_logs)
                assert report.stdout == expected_http and not report.stderr
            report_path = root/'existing-http.jsonl'
            report_path.write_bytes(expected_http)
            invalid = run(['data', '--recipe', report_recipe, '-o', report_path, '--force'],
                          http_logs + b'{"status":501,"error":{"code":null}}\n', success=False)
            assert not invalid.stdout and report_path.read_bytes() == expected_http

            http_name = 'builtin:top-http-5xx-codes'
            custom_http_args = ['data', '--recipe', http_name, '--var', 'status_pointer=/response/status',
                                '--var', 'code_pointer=/fault/id']
            custom_http = (
                b'{"response":{"status":499},"fault":{"id":"OUTSIDE"}}\n'
                b'{"response":{"status":500},"fault":{"id":"TIMEOUT"}}\n'
                b'{"response":{"status":599},"fault":{"id":"TIMEOUT"}}\n'
                b'{"response":{"status":502},"fault":{"id":"BAD_GATEWAY"}}\n'
                b'{"response":{"status":600},"fault":{"id":"OUTSIDE"}}\n'
            )
            all_http = run(custom_http_args, custom_http)
            assert all_http.stdout == expected_http and not all_http.stderr
            limited_http = [*custom_http_args, '--var', 'limit=1']
            expected_custom_http = b'{"value":"TIMEOUT","count":2}\n'
            ranked = run(limited_http, custom_http)
            assert ranked.stdout == expected_custom_http and not ranked.stderr
            resolved_http = run([*limited_http, '--show-plan'])
            assert not resolved_http.stderr and resolved_http.stdout.startswith(b'version = 1\n')
            custom_http_plan = root/'custom HTTP report-测试.toml'
            custom_http_plan.write_bytes(resolved_http.stdout)
            assert run(['data', '--recipe', custom_http_plan], custom_http).stdout == expected_custom_http

            app_name = 'builtin:top-error-codes'
            custom_app_args = ['data', '--recipe', app_name, '--var', 'level_pointer=/event/severity',
                               '--var', 'code_pointer=/fault/id']
            custom_app = (
                b'{"event":{"severity":"WARN"},"fault":{"id":"IGNORED"}}\n'
                b'{"event":{"severity":"error"},"fault":{"id":"IGNORED"}}\n'
                b'{"event":{"severity":"ERROR"},"fault":{"id":"DISK"}}\n'
                b'{"event":{"severity":"ERROR"},"fault":{"id":"NETWORK"}}\n'
                b'{"event":{"severity":"ERROR"},"fault":{"id":"DISK"}}\n'
            )
            all_app = run(custom_app_args, custom_app)
            assert all_app.stdout == b'{"value":"DISK","count":2}\n{"value":"NETWORK","count":1}\n'
            assert not all_app.stderr
            ranked = run([*custom_app_args, '--var', 'limit=1'], custom_app)
            assert ranked.stdout == b'{"value":"DISK","count":2}\n' and not ranked.stderr
            for name, selector in [(http_name, 'status_pointer'), (app_name, 'level_pointer')]:
                declared = run(['data', '--list-parameters', '--recipe', name])
                parameters = json.loads(declared.stdout)['parameters']
                assert not declared.stderr and set(parameters) == {selector, 'code_pointer', 'limit'}
                assert parameters[selector]['type'] == parameters['code_pointer']['type'] == 'text'
                assert parameters['limit']['type'] == 'line_count'
                assert all(parameter['required'] is False for parameter in parameters.values())
            checks.append('plan export: table replay, line context and default/custom log rankings')

            log_context = 'builtin:log-context'
            declared = run(['data', '--list-parameters', '--recipe', log_context])
            assert not declared.stderr
            parameters = json.loads(declared.stdout)
            assert parameters['recipe_version'] == 2
            assert parameters['parameters']['needle']['required'] is True
            assert parameters['parameters']['nearby']['required'] is False
            team_recipe = root/'team 日志.toml'
            copied = run(['data', '--export-recipe', '--recipe', log_context, '-o', team_recipe])
            assert not copied.stdout and not copied.stderr
            original_template = team_recipe.read_bytes()
            assert b'version = 2' in original_template and b'bind = ' in original_template
            customized = original_template.replace(b'context = 1', b'context = 0', 1)
            assert customized != original_template
            team_recipe.write_bytes(customized)
            log_input = 'before\r\n请求超时\r\nretry\r\n'.encode()
            expected_log = '请求超时\n'.encode()
            variable = ['--var', 'needle=超时']
            direct = run(['data', '--recipe', team_recipe, *variable], log_input)
            assert direct.stdout == expected_log and not direct.stderr
            plan = run(['data', '--show-plan', '--recipe', team_recipe, *variable])
            assert not plan.stderr and plan.stdout.startswith(b'version = 1\n')
            replay = root/'reviewed 日志.toml'
            replay.write_bytes(plan.stdout)
            replayed = run(['data', '--recipe', replay], log_input)
            assert replayed.stdout == expected_log and not replayed.stderr
            checks.append('installed recipe reuse: discover, copy, customize and replay offline')

            projected = run(['data', 'jsonl-project', '--field', 'tag=/tag',
                             '--field-default', 'tag=""'], b'{"tag":null}\n{}\n')
            assert projected.stdout == b'{"tag":null}\n{"tag":""}\n' and not projected.stderr
            checks.append('default: present null retained, absent value explicitly filled')
            sentinel = b'previous complete result\n'
            destination.write_bytes(sentinel)
            for name, args, data in [
                ('selected noninteger', ['data', 'jsonl-where-int', '--pointer', '/n', '--equals', '2'],
                 b'{"n":2}\n{"n":2.0}\n'),
                ('malformed original Unicode', ['data', 'jsonl-project', '--field', 'x=/missing', '--field-default', 'x=null'],
                 b'{}\n{"private":"\\ud800"}\n'),
                ('invalid literal before input', ['data', 'jsonl-project', '--field', 'x=/missing',
                 '--field-default', 'x=PRIVATE_LITERAL', '-i', 'DO_NOT_READ'], b''),
            ]:
                result = run([*args, '-o', destination, '--force'], data, success=False)
                assert not result.stdout and destination.read_bytes() == sentinel
                assert b'PRIVATE_LITERAL' not in result.stderr
                if name == 'invalid literal before input':
                    assert b'cannot open DO_NOT_READ' not in result.stderr
                checks.append(name+': no prefix output, old file preserved')
            assert not (root/'config').exists()
    return checks


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    args = parser.parse_args()
    checked = verify(args.binary.resolve(strict=True))
    print(json.dumps({'passed': len(checked), 'checks': checked}, ensure_ascii=True))
